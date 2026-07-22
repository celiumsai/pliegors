// SPDX-License-Identifier: Apache-2.0

use crate::receipt::{DataDurationBucket, DataOperation, DataOutcome};
use crate::{DataContext, DataError, DataReceipt, DataSizeBucket, validate_stable_id};
use std::collections::BTreeSet;
use std::fmt::{Debug, Display, Formatter};
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use url::{Host, Url};

pub type OutboundFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

pub trait OutboundDnsResolver: Send + Sync + 'static {
    fn resolve(
        &self,
        host: String,
        port: u16,
    ) -> OutboundFuture<Result<Vec<SocketAddr>, OutboundHttpError>>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemDnsResolver;

impl OutboundDnsResolver for SystemDnsResolver {
    fn resolve(
        &self,
        host: String,
        port: u16,
    ) -> OutboundFuture<Result<Vec<SocketAddr>, OutboundHttpError>> {
        Box::pin(async move {
            Ok(tokio::net::lookup_host((host.as_str(), port))
                .await
                .map_err(|_| OutboundHttpError::DnsFailure)?
                .collect())
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboundHttpPolicy {
    id: String,
    semantic_revision: u32,
    allowed_hosts: BTreeSet<String>,
    allowed_ports: BTreeSet<u16>,
    allowed_path_prefixes: BTreeSet<String>,
    allow_http: bool,
    allow_private_networks: bool,
    timeout: Duration,
    max_redirects: u8,
    max_response_bytes: usize,
    max_resolved_addresses: usize,
}

impl OutboundHttpPolicy {
    pub fn new(id: impl Into<String>, semantic_revision: u32) -> Result<Self, OutboundHttpError> {
        let id = id.into();
        if !validate_stable_id(&id) || semantic_revision == 0 {
            return Err(OutboundHttpError::InvalidPolicy);
        }
        Ok(Self {
            id,
            semantic_revision,
            allowed_hosts: BTreeSet::new(),
            allowed_ports: BTreeSet::from([443]),
            allowed_path_prefixes: BTreeSet::from(["/".to_owned()]),
            allow_http: false,
            allow_private_networks: false,
            timeout: Duration::from_secs(5),
            max_redirects: 3,
            max_response_bytes: 8 * 1_024 * 1_024,
            max_resolved_addresses: 16,
        })
    }

    pub fn allow_host(mut self, host: impl AsRef<str>) -> Result<Self, OutboundHttpError> {
        if self.allowed_hosts.len() >= 64 {
            return Err(OutboundHttpError::InvalidPolicy);
        }
        self.allowed_hosts.insert(normalize_host(host.as_ref())?);
        Ok(self)
    }

    pub fn allow_port(mut self, port: u16) -> Result<Self, OutboundHttpError> {
        if port == 0 || self.allowed_ports.len() >= 16 {
            return Err(OutboundHttpError::InvalidPolicy);
        }
        self.allowed_ports.insert(port);
        Ok(self)
    }

    pub fn restrict_path_prefix(
        mut self,
        prefix: impl Into<String>,
    ) -> Result<Self, OutboundHttpError> {
        let prefix = prefix.into();
        if prefix.is_empty()
            || !prefix.starts_with('/')
            || (prefix != "/" && !prefix.ends_with('/'))
            || prefix.contains('%')
            || prefix.contains('?')
            || prefix.contains('#')
            || prefix.contains("//")
            || prefix
                .split('/')
                .any(|segment| matches!(segment, "." | ".."))
            || prefix.chars().any(char::is_control)
        {
            return Err(OutboundHttpError::InvalidPolicy);
        }
        if self.allowed_path_prefixes.len() == 1
            && self.allowed_path_prefixes.contains("/")
            && prefix != "/"
        {
            self.allowed_path_prefixes.clear();
        }
        if self.allowed_path_prefixes.len() >= 32 {
            return Err(OutboundHttpError::InvalidPolicy);
        }
        self.allowed_path_prefixes.insert(prefix);
        Ok(self)
    }

    pub fn allow_plain_http(mut self, allowed: bool) -> Self {
        self.allow_http = allowed;
        self
    }

    pub fn allow_private_networks(mut self, allowed: bool) -> Self {
        self.allow_private_networks = allowed;
        self
    }

    pub fn limits(
        mut self,
        timeout: Duration,
        max_redirects: u8,
        max_response_bytes: usize,
        max_resolved_addresses: usize,
    ) -> Result<Self, OutboundHttpError> {
        if timeout.is_zero()
            || timeout > Duration::from_secs(60)
            || max_redirects > 10
            || max_response_bytes == 0
            || max_response_bytes > 64 * 1_024 * 1_024
            || max_resolved_addresses == 0
            || max_resolved_addresses > 64
        {
            return Err(OutboundHttpError::InvalidPolicy);
        }
        self.timeout = timeout;
        self.max_redirects = max_redirects;
        self.max_response_bytes = max_response_bytes;
        self.max_resolved_addresses = max_resolved_addresses;
        Ok(self)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn semantic_revision(&self) -> u32 {
        self.semantic_revision
    }
}

#[derive(Clone)]
pub struct OutboundHttpGuard<Resolver> {
    policy: OutboundHttpPolicy,
    resolver: Arc<Resolver>,
}

impl<Resolver> OutboundHttpGuard<Resolver>
where
    Resolver: OutboundDnsResolver,
{
    pub fn new(policy: OutboundHttpPolicy, resolver: Resolver) -> Result<Self, OutboundHttpError> {
        if policy.allowed_hosts.is_empty() {
            return Err(OutboundHttpError::InvalidPolicy);
        }
        Ok(Self {
            policy,
            resolver: Arc::new(resolver),
        })
    }

    pub fn policy(&self) -> &OutboundHttpPolicy {
        &self.policy
    }

    pub async fn authorize(
        &self,
        context: &DataContext,
        url: &str,
    ) -> Result<OutboundHttpPermit, OutboundHttpError> {
        self.authorize_with_redirect_budget(context, url, self.policy.max_redirects)
            .await
    }

    pub async fn authorize_redirect(
        &self,
        context: &DataContext,
        previous: &OutboundHttpPermit,
        location: &str,
    ) -> Result<OutboundHttpPermit, OutboundHttpError> {
        if previous.policy_id != self.policy.id
            || previous.semantic_revision != self.policy.semantic_revision
        {
            return Err(OutboundHttpError::PolicyMismatch);
        }
        if previous.redirects_remaining == 0 {
            return Err(OutboundHttpError::RedirectLimit);
        }
        let base = Url::parse(&previous.url).map_err(|_| OutboundHttpError::InvalidUrl)?;
        let next = base
            .join(location)
            .map_err(|_| OutboundHttpError::InvalidUrl)?;
        self.authorize_with_redirect_budget(
            context,
            next.as_str(),
            previous.redirects_remaining - 1,
        )
        .await
    }

    async fn authorize_with_redirect_budget(
        &self,
        context: &DataContext,
        authored: &str,
        redirects_remaining: u8,
    ) -> Result<OutboundHttpPermit, OutboundHttpError> {
        let started = Instant::now();
        let result = self
            .authorize_inner(context, authored, redirects_remaining)
            .await;
        let (outcome, diagnostic_code) = match &result {
            Ok(_) => (DataOutcome::Success, None),
            Err(OutboundHttpError::Cancelled | OutboundHttpError::Deadline) => (
                DataOutcome::Cancelled,
                result.as_ref().err().map(|error| error.code().to_owned()),
            ),
            Err(error) => (DataOutcome::Rejected, Some(error.code().to_owned())),
        };
        context.record_receipt(DataReceipt {
            contract: "dev.pliegors.data-receipt/v1".to_owned(),
            operation: DataOperation::OutboundHttp,
            operation_id: self.policy.id.clone(),
            semantic_revision: self.policy.semantic_revision,
            outcome,
            duration_bucket: DataDurationBucket::from_duration(started.elapsed()),
            output_size_bucket: DataSizeBucket::None,
            deduplicated: false,
            cancel_reason: context.cancel_reason(),
            diagnostic_code,
        });
        result
    }

    async fn authorize_inner(
        &self,
        context: &DataContext,
        authored: &str,
        redirects_remaining: u8,
    ) -> Result<OutboundHttpPermit, OutboundHttpError> {
        if context.is_closed() {
            return Err(OutboundHttpError::ContextClosed);
        }
        if context.cancellation().is_cancelled() {
            return Err(OutboundHttpError::Cancelled);
        }
        let now = Instant::now();
        if now >= context.deadline() {
            return Err(OutboundHttpError::Deadline);
        }
        let parsed = Url::parse(authored).map_err(|_| OutboundHttpError::InvalidUrl)?;
        if parsed.username() != "" || parsed.password().is_some() || parsed.fragment().is_some() {
            return Err(OutboundHttpError::ForbiddenUrlComponent);
        }
        match parsed.scheme() {
            "https" => {}
            "http" if self.policy.allow_http => {}
            _ => return Err(OutboundHttpError::SchemeRejected),
        }
        let host = parsed.host().ok_or(OutboundHttpError::MissingHost)?;
        let normalized_host = normalize_parsed_host(&host);
        if !self.policy.allowed_hosts.contains(&normalized_host) {
            return Err(OutboundHttpError::HostRejected);
        }
        let port = parsed
            .port_or_known_default()
            .ok_or(OutboundHttpError::PortRejected)?;
        if !self.policy.allowed_ports.contains(&port) {
            return Err(OutboundHttpError::PortRejected);
        }
        if !path_is_allowed(parsed.path(), &self.policy.allowed_path_prefixes) {
            return Err(OutboundHttpError::PathRejected);
        }

        let addresses = match host {
            Host::Ipv4(address) => vec![SocketAddr::new(IpAddr::V4(address), port)],
            Host::Ipv6(address) => vec![SocketAddr::new(IpAddr::V6(address), port)],
            Host::Domain(_) => {
                let remaining = context.deadline().saturating_duration_since(now);
                let timeout = self.policy.timeout.min(remaining);
                let resolution = self.resolver.resolve(normalized_host.clone(), port);
                tokio::select! {
                    biased;
                    _ = context.cancellation().cancelled() => return Err(OutboundHttpError::Cancelled),
                    result = tokio::time::timeout(timeout, resolution) => {
                        result.map_err(|_| OutboundHttpError::Deadline)??
                    }
                }
            }
        };
        if addresses.is_empty() || addresses.len() > self.policy.max_resolved_addresses {
            return Err(OutboundHttpError::DnsAddressCount);
        }
        let unique = addresses.into_iter().collect::<BTreeSet<_>>();
        if unique.iter().any(|address| address.port() != port) {
            return Err(OutboundHttpError::DnsAddressCount);
        }
        if !self.policy.allow_private_networks
            && unique.iter().any(|address| !is_public_ip(address.ip()))
        {
            return Err(OutboundHttpError::PrivateAddressRejected);
        }
        let expires_at = now
            + self
                .policy
                .timeout
                .min(context.deadline().duration_since(now));
        Ok(OutboundHttpPermit {
            policy_id: self.policy.id.clone(),
            semantic_revision: self.policy.semantic_revision,
            url: parsed.to_string(),
            host: normalized_host,
            addresses: unique.into_iter().collect(),
            expires_at,
            redirects_remaining,
            max_response_bytes: self.policy.max_response_bytes,
        })
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct OutboundHttpPermit {
    policy_id: String,
    semantic_revision: u32,
    url: String,
    host: String,
    addresses: Vec<SocketAddr>,
    expires_at: Instant,
    redirects_remaining: u8,
    max_response_bytes: usize,
}

impl OutboundHttpPermit {
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn resolved_addresses(&self) -> &[SocketAddr] {
        &self.addresses
    }

    pub fn expires_at(&self) -> Instant {
        self.expires_at
    }

    pub fn redirects_remaining(&self) -> u8 {
        self.redirects_remaining
    }

    pub fn max_response_bytes(&self) -> usize {
        self.max_response_bytes
    }

    pub fn ensure_live(&self) -> Result<(), OutboundHttpError> {
        if Instant::now() >= self.expires_at {
            Err(OutboundHttpError::Deadline)
        } else {
            Ok(())
        }
    }
}

impl Debug for OutboundHttpPermit {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OutboundHttpPermit")
            .field("policy_id", &self.policy_id)
            .field("semantic_revision", &self.semantic_revision)
            .field("host", &self.host)
            .field("address_count", &self.addresses.len())
            .field("redirects_remaining", &self.redirects_remaining)
            .field("max_response_bytes", &self.max_response_bytes)
            .finish()
    }
}

fn normalize_host(value: &str) -> Result<String, OutboundHttpError> {
    let host = Host::parse(value).map_err(|_| OutboundHttpError::InvalidPolicy)?;
    Ok(normalize_parsed_host(&host))
}

fn path_is_allowed(path: &str, prefixes: &BTreeSet<String>) -> bool {
    if prefixes.contains("/") {
        return true;
    }
    if path.contains('%') || path.contains('\\') || path.contains("//") {
        return false;
    }
    prefixes
        .iter()
        .any(|prefix| path == prefix.trim_end_matches('/') || path.starts_with(prefix.as_str()))
}

fn normalize_parsed_host<S>(host: &Host<S>) -> String
where
    S: AsRef<str>,
{
    match host {
        Host::Domain(value) => value.as_ref().to_ascii_lowercase(),
        Host::Ipv4(value) => value.to_string(),
        Host::Ipv6(value) => value.to_string(),
    }
}

fn is_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_ipv4(address),
        IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                return is_public_ipv4(mapped);
            }
            let segments = address.segments();
            !address.is_unspecified()
                && !address.is_loopback()
                && !address.is_multicast()
                && segments[0] & 0xfe00 != 0xfc00
                && segments[0] & 0xffc0 != 0xfe80
                && segments[0] & 0xffc0 != 0xfec0
                && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
        }
    }
}

fn is_public_ipv4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    !(address.is_unspecified()
        || address.is_loopback()
        || address.is_private()
        || address.is_link_local()
        || address.is_multicast()
        || address.is_broadcast()
        || a == 0
        || (a == 100 && (64..=127).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 240)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutboundHttpError {
    InvalidPolicy,
    InvalidUrl,
    ForbiddenUrlComponent,
    SchemeRejected,
    MissingHost,
    HostRejected,
    PortRejected,
    PathRejected,
    DnsFailure,
    DnsAddressCount,
    PrivateAddressRejected,
    RedirectLimit,
    PolicyMismatch,
    ContextClosed,
    Cancelled,
    Deadline,
}

impl OutboundHttpError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPolicy => "PLG-HTTP-001",
            Self::InvalidUrl | Self::ForbiddenUrlComponent | Self::MissingHost => "PLG-HTTP-101",
            Self::SchemeRejected | Self::HostRejected | Self::PortRejected => "PLG-HTTP-102",
            Self::PathRejected => "PLG-HTTP-106",
            Self::DnsFailure | Self::DnsAddressCount => "PLG-HTTP-103",
            Self::PrivateAddressRejected => "PLG-HTTP-104",
            Self::RedirectLimit => "PLG-HTTP-105",
            Self::PolicyMismatch => "PLG-HTTP-409",
            Self::ContextClosed => "PLG-HTTP-410",
            Self::Cancelled | Self::Deadline => "PLG-HTTP-408",
        }
    }
}

impl Display for OutboundHttpError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPolicy => "outbound HTTP policy is invalid",
            Self::InvalidUrl => "outbound URL is invalid",
            Self::ForbiddenUrlComponent => "outbound URL contains credentials or a fragment",
            Self::SchemeRejected => "outbound URL scheme is not allowed",
            Self::MissingHost => "outbound URL has no host",
            Self::HostRejected => "outbound host is not allowlisted",
            Self::PortRejected => "outbound port is not allowlisted",
            Self::PathRejected => "outbound path is not allowlisted",
            Self::DnsFailure => "outbound DNS resolution failed",
            Self::DnsAddressCount => "outbound DNS result exceeded its address policy",
            Self::PrivateAddressRejected => "outbound address is private or reserved",
            Self::RedirectLimit => "outbound redirect limit was reached",
            Self::PolicyMismatch => "outbound permit belongs to a different policy",
            Self::ContextClosed => "request context is closed",
            Self::Cancelled => "outbound authorization was cancelled",
            Self::Deadline => "outbound authorization exceeded its deadline",
        })
    }
}

impl std::error::Error for OutboundHttpError {}

impl From<OutboundHttpError> for DataError {
    fn from(error: OutboundHttpError) -> Self {
        DataError::LoaderFailure(format!("{}: {error}", error.code()))
    }
}
