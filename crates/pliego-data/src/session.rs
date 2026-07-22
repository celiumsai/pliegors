// SPDX-License-Identifier: Apache-2.0

use crate::validate_stable_id;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SESSION_TOKEN_BYTES: usize = 32;
const SESSION_TOKEN_TEXT_BYTES: usize = 43;

pub type SessionFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SameSitePolicy {
    Strict,
    Lax,
    None,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionCookiePolicy {
    name: String,
    path: String,
    secure: bool,
    http_only: bool,
    same_site: SameSitePolicy,
}

impl Default for SessionCookiePolicy {
    fn default() -> Self {
        Self {
            name: "__Host-pliego-session".to_owned(),
            path: "/".to_owned(),
            secure: true,
            http_only: true,
            same_site: SameSitePolicy::Lax,
        }
    }
}

impl SessionCookiePolicy {
    pub fn new(name: impl Into<String>) -> Result<Self, SessionError> {
        let name = name.into();
        validate_cookie_name(&name)?;
        Ok(Self {
            name,
            ..Self::default()
        })
    }

    pub fn path(mut self, path: impl Into<String>) -> Result<Self, SessionError> {
        let path = path.into();
        if path.is_empty()
            || !path.starts_with('/')
            || path.contains(';')
            || path.chars().any(char::is_control)
            || (self.name.starts_with("__Host-") && path != "/")
        {
            return Err(SessionError::InvalidPolicy(
                "invalid cookie path".to_owned(),
            ));
        }
        self.path = path;
        Ok(self)
    }

    pub fn secure(mut self, secure: bool) -> Result<Self, SessionError> {
        if !secure
            && (self.name.starts_with("__Host-")
                || self.name.starts_with("__Secure-")
                || self.same_site == SameSitePolicy::None)
        {
            return Err(SessionError::InvalidPolicy(
                "cookie policy requires Secure".to_owned(),
            ));
        }
        self.secure = secure;
        Ok(self)
    }

    pub fn http_only(mut self, http_only: bool) -> Self {
        self.http_only = http_only;
        self
    }

    pub fn same_site(mut self, same_site: SameSitePolicy) -> Result<Self, SessionError> {
        if same_site == SameSitePolicy::None && !self.secure {
            return Err(SessionError::InvalidPolicy(
                "SameSite=None requires Secure".to_owned(),
            ));
        }
        self.same_site = same_site;
        Ok(self)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn path_value(&self) -> &str {
        &self.path
    }

    pub fn is_secure(&self) -> bool {
        self.secure
    }

    pub fn is_http_only(&self) -> bool {
        self.http_only
    }

    pub fn same_site_value(&self) -> SameSitePolicy {
        self.same_site
    }
}

fn validate_cookie_name(name: &str) -> Result<(), SessionError> {
    if name.is_empty()
        || name.len() > 128
        || !name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
    {
        return Err(SessionError::InvalidPolicy(
            "invalid cookie name".to_owned(),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionPolicy {
    id: String,
    schema_version: u32,
    cookie: SessionCookiePolicy,
    idle_timeout: Duration,
    absolute_timeout: Duration,
    max_claim_bytes: usize,
    assurance: String,
}

impl SessionPolicy {
    pub fn new(id: impl Into<String>, schema_version: u32) -> Result<Self, SessionError> {
        let id = id.into();
        if !validate_stable_id(&id) {
            return Err(SessionError::InvalidPolicy(
                "invalid session policy ID".to_owned(),
            ));
        }
        if schema_version == 0 {
            return Err(SessionError::InvalidPolicy(
                "schema version must be greater than zero".to_owned(),
            ));
        }
        Ok(Self {
            id,
            schema_version,
            cookie: SessionCookiePolicy::default(),
            idle_timeout: Duration::from_secs(30 * 60),
            absolute_timeout: Duration::from_secs(24 * 60 * 60),
            max_claim_bytes: 4 * 1_024,
            assurance: "application".to_owned(),
        })
    }

    pub fn cookie(mut self, cookie: SessionCookiePolicy) -> Self {
        self.cookie = cookie;
        self
    }

    pub fn timeouts(mut self, idle: Duration, absolute: Duration) -> Result<Self, SessionError> {
        if idle.is_zero()
            || absolute.is_zero()
            || idle > absolute
            || absolute > Duration::from_secs(365 * 24 * 60 * 60)
        {
            return Err(SessionError::InvalidPolicy(
                "session timeouts are invalid".to_owned(),
            ));
        }
        self.idle_timeout = idle;
        self.absolute_timeout = absolute;
        Ok(self)
    }

    pub fn max_claim_bytes(mut self, maximum: usize) -> Result<Self, SessionError> {
        if maximum == 0 || maximum > 64 * 1_024 {
            return Err(SessionError::InvalidPolicy(
                "max claim bytes must be between 1 and 65536".to_owned(),
            ));
        }
        self.max_claim_bytes = maximum;
        Ok(self)
    }

    pub fn assurance(mut self, assurance: impl Into<String>) -> Result<Self, SessionError> {
        let assurance = assurance.into();
        if !validate_stable_id(&assurance) {
            return Err(SessionError::InvalidPolicy(
                "invalid assurance ID".to_owned(),
            ));
        }
        self.assurance = assurance;
        Ok(self)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn cookie_policy(&self) -> &SessionCookiePolicy {
        &self.cookie
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct SessionToken(String);

impl SessionToken {
    fn random() -> Result<Self, SessionError> {
        let mut bytes = [0_u8; SESSION_TOKEN_BYTES];
        getrandom::fill(&mut bytes).map_err(|_| SessionError::EntropyUnavailable)?;
        Ok(Self(URL_SAFE_NO_PAD.encode(bytes)))
    }

    pub fn parse(value: &str) -> Result<Self, SessionError> {
        if value.len() != SESSION_TOKEN_TEXT_BYTES
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(SessionError::InvalidToken);
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| SessionError::InvalidToken)?;
        if bytes.len() != SESSION_TOKEN_BYTES {
            return Err(SessionError::InvalidToken);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_cookie_value(&self) -> &str {
        &self.0
    }

    pub fn digest(&self) -> String {
        encode_hex(&Sha256::digest(self.0.as_bytes()))
    }
}

impl Debug for SessionToken {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SessionToken([REDACTED])")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionCookie {
    token: SessionToken,
    policy: SessionCookiePolicy,
    max_age: Duration,
}

impl SessionCookie {
    pub fn token(&self) -> &SessionToken {
        &self.token
    }

    pub fn policy(&self) -> &SessionCookiePolicy {
        &self.policy
    }

    pub fn max_age(&self) -> Duration {
        self.max_age
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedSession<Claims> {
    pub token_digest: String,
    pub schema_version: u32,
    pub assurance: String,
    pub claims: Claims,
    pub created_at_ms: u64,
    pub rotated_at_ms: u64,
    pub last_seen_at_ms: u64,
    pub idle_expires_at_ms: u64,
    pub absolute_expires_at_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatedSession<Claims> {
    pub session: LoadedSession<Claims>,
    pub cookie: SessionCookie,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct StoredSession {
    policy_id: String,
    schema_version: u32,
    assurance: String,
    claims: Vec<u8>,
    created_at_ms: u64,
    rotated_at_ms: u64,
    last_seen_at_ms: u64,
    idle_expires_at_ms: u64,
    absolute_expires_at_ms: u64,
}

impl StoredSession {
    pub fn encode(&self) -> Result<Vec<u8>, SessionError> {
        serde_json::to_vec(self)
            .map_err(|_| SessionError::StoreFailure("session record encoding failed".to_owned()))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, SessionError> {
        serde_json::from_slice(bytes)
            .map_err(|_| SessionError::StoreFailure("session record decoding failed".to_owned()))
    }
}

pub trait SessionStore: Send + Sync + 'static {
    fn create(
        &self,
        token: SessionToken,
        session: StoredSession,
    ) -> SessionFuture<Result<(), SessionError>>;
    fn read(
        &self,
        token: SessionToken,
    ) -> SessionFuture<Result<Option<StoredSession>, SessionError>>;
    fn replace(
        &self,
        old_token: SessionToken,
        new_token: SessionToken,
        session: StoredSession,
    ) -> SessionFuture<Result<bool, SessionError>>;
    fn update(
        &self,
        token: SessionToken,
        session: StoredSession,
    ) -> SessionFuture<Result<bool, SessionError>>;
    fn revoke(&self, token: SessionToken) -> SessionFuture<Result<bool, SessionError>>;
}

#[derive(Clone, Default)]
pub struct InMemorySessionStore {
    entries: Arc<Mutex<BTreeMap<String, StoredSession>>>,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        lock(&self.entries).len()
    }

    pub fn is_empty(&self) -> bool {
        lock(&self.entries).is_empty()
    }
}

impl SessionStore for InMemorySessionStore {
    fn create(
        &self,
        token: SessionToken,
        session: StoredSession,
    ) -> SessionFuture<Result<(), SessionError>> {
        let entries = self.entries.clone();
        Box::pin(async move {
            let mut entries = lock(&entries);
            if entries.insert(token.digest(), session).is_some() {
                return Err(SessionError::StoreConflict);
            }
            Ok(())
        })
    }

    fn read(
        &self,
        token: SessionToken,
    ) -> SessionFuture<Result<Option<StoredSession>, SessionError>> {
        let entries = self.entries.clone();
        Box::pin(async move { Ok(lock(&entries).get(&token.digest()).cloned()) })
    }

    fn replace(
        &self,
        old_token: SessionToken,
        new_token: SessionToken,
        session: StoredSession,
    ) -> SessionFuture<Result<bool, SessionError>> {
        let entries = self.entries.clone();
        Box::pin(async move {
            let mut entries = lock(&entries);
            let old_key = old_token.digest();
            let new_key = new_token.digest();
            if !entries.contains_key(&old_key) {
                return Ok(false);
            }
            if entries.contains_key(&new_key) {
                return Err(SessionError::StoreConflict);
            }
            entries.remove(&old_key);
            entries.insert(new_key, session);
            Ok(true)
        })
    }

    fn update(
        &self,
        token: SessionToken,
        session: StoredSession,
    ) -> SessionFuture<Result<bool, SessionError>> {
        let entries = self.entries.clone();
        Box::pin(async move {
            let mut entries = lock(&entries);
            let key = token.digest();
            if !entries.contains_key(&key) {
                return Ok(false);
            }
            entries.insert(key, session);
            Ok(true)
        })
    }

    fn revoke(&self, token: SessionToken) -> SessionFuture<Result<bool, SessionError>> {
        let entries = self.entries.clone();
        Box::pin(async move { Ok(lock(&entries).remove(&token.digest()).is_some()) })
    }
}

#[derive(Clone)]
pub struct SessionManager<Store> {
    policy: SessionPolicy,
    store: Arc<Store>,
}

impl<Store> SessionManager<Store>
where
    Store: SessionStore,
{
    pub fn new(policy: SessionPolicy, store: Store) -> Self {
        Self {
            policy,
            store: Arc::new(store),
        }
    }

    pub fn policy(&self) -> &SessionPolicy {
        &self.policy
    }

    pub async fn create<Claims>(
        &self,
        claims: Claims,
    ) -> Result<CreatedSession<Claims>, SessionError>
    where
        Claims: Clone + Serialize + DeserializeOwned,
    {
        let now = unix_millis()?;
        let token = SessionToken::random()?;
        let stored = self.stored(claims.clone(), now, now, now)?;
        self.store.create(token.clone(), stored.clone()).await?;
        Ok(CreatedSession {
            session: loaded(&token, &stored, claims),
            cookie: self.cookie(token),
        })
    }

    pub async fn load<Claims>(
        &self,
        token: &SessionToken,
    ) -> Result<Option<LoadedSession<Claims>>, SessionError>
    where
        Claims: DeserializeOwned,
    {
        let Some(mut stored) = self.store.read(token.clone()).await? else {
            return Ok(None);
        };
        let now = unix_millis()?;
        if stored.policy_id != self.policy.id || stored.schema_version != self.policy.schema_version
        {
            return Err(SessionError::VersionMismatch);
        }
        if now >= stored.idle_expires_at_ms || now >= stored.absolute_expires_at_ms {
            let _ = self.store.revoke(token.clone()).await;
            return Ok(None);
        }
        stored.last_seen_at_ms = now;
        stored.idle_expires_at_ms = now
            .saturating_add(duration_millis(self.policy.idle_timeout)?)
            .min(stored.absolute_expires_at_ms);
        if !self.store.update(token.clone(), stored.clone()).await? {
            return Ok(None);
        }
        let claims =
            serde_json::from_slice(&stored.claims).map_err(|_| SessionError::InvalidClaims)?;
        Ok(Some(loaded(token, &stored, claims)))
    }

    pub async fn rotate<Claims>(
        &self,
        old_token: &SessionToken,
        claims: Claims,
    ) -> Result<Option<CreatedSession<Claims>>, SessionError>
    where
        Claims: Clone + Serialize + DeserializeOwned,
    {
        let Some(existing) = self.store.read(old_token.clone()).await? else {
            return Ok(None);
        };
        let now = unix_millis()?;
        if existing.policy_id != self.policy.id
            || existing.schema_version != self.policy.schema_version
            || now >= existing.idle_expires_at_ms
            || now >= existing.absolute_expires_at_ms
        {
            let _ = self.store.revoke(old_token.clone()).await;
            return Ok(None);
        }
        let new_token = SessionToken::random()?;
        let mut stored = self.stored(claims.clone(), existing.created_at_ms, now, now)?;
        stored.absolute_expires_at_ms = existing.absolute_expires_at_ms;
        if !self
            .store
            .replace(old_token.clone(), new_token.clone(), stored.clone())
            .await?
        {
            return Ok(None);
        }
        Ok(Some(CreatedSession {
            session: loaded(&new_token, &stored, claims),
            cookie: self.cookie(new_token),
        }))
    }

    pub async fn revoke(&self, token: &SessionToken) -> Result<bool, SessionError> {
        self.store.revoke(token.clone()).await
    }

    fn stored<Claims>(
        &self,
        claims: Claims,
        created_at_ms: u64,
        rotated_at_ms: u64,
        last_seen_at_ms: u64,
    ) -> Result<StoredSession, SessionError>
    where
        Claims: Serialize,
    {
        let claims = serde_json::to_vec(&claims).map_err(|_| SessionError::InvalidClaims)?;
        if claims.len() > self.policy.max_claim_bytes {
            return Err(SessionError::ClaimsTooLarge {
                actual: claims.len(),
                maximum: self.policy.max_claim_bytes,
            });
        }
        let idle_ms = duration_millis(self.policy.idle_timeout)?;
        let absolute_ms = duration_millis(self.policy.absolute_timeout)?;
        Ok(StoredSession {
            policy_id: self.policy.id.clone(),
            schema_version: self.policy.schema_version,
            assurance: self.policy.assurance.clone(),
            claims,
            created_at_ms,
            rotated_at_ms,
            last_seen_at_ms,
            idle_expires_at_ms: last_seen_at_ms.saturating_add(idle_ms),
            absolute_expires_at_ms: created_at_ms.saturating_add(absolute_ms),
        })
    }

    fn cookie(&self, token: SessionToken) -> SessionCookie {
        SessionCookie {
            token,
            policy: self.policy.cookie.clone(),
            max_age: self.policy.absolute_timeout,
        }
    }
}

fn loaded<Claims>(
    token: &SessionToken,
    stored: &StoredSession,
    claims: Claims,
) -> LoadedSession<Claims> {
    LoadedSession {
        token_digest: token.digest(),
        schema_version: stored.schema_version,
        assurance: stored.assurance.clone(),
        claims,
        created_at_ms: stored.created_at_ms,
        rotated_at_ms: stored.rotated_at_ms,
        last_seen_at_ms: stored.last_seen_at_ms,
        idle_expires_at_ms: stored.idle_expires_at_ms,
        absolute_expires_at_ms: stored.absolute_expires_at_ms,
    }
}

fn unix_millis() -> Result<u64, SessionError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SessionError::Clock)?
        .as_millis();
    u64::try_from(millis).map_err(|_| SessionError::Clock)
}

fn duration_millis(duration: Duration) -> Result<u64, SessionError> {
    u64::try_from(duration.as_millis())
        .map_err(|_| SessionError::InvalidPolicy("duration is too large".to_owned()))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionError {
    InvalidPolicy(String),
    InvalidToken,
    EntropyUnavailable,
    InvalidClaims,
    ClaimsTooLarge { actual: usize, maximum: usize },
    VersionMismatch,
    StoreConflict,
    StoreFailure(String),
    Clock,
}

impl SessionError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPolicy(_) => "PLG-SES-001",
            Self::InvalidToken => "PLG-SES-101",
            Self::EntropyUnavailable => "PLG-SES-500",
            Self::InvalidClaims | Self::ClaimsTooLarge { .. } => "PLG-SES-201",
            Self::VersionMismatch => "PLG-SES-409",
            Self::StoreConflict => "PLG-SES-409",
            Self::StoreFailure(_) | Self::Clock => "PLG-SES-500",
        }
    }
}

impl Display for SessionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPolicy(message) => write!(formatter, "invalid session policy: {message}"),
            Self::InvalidToken => formatter.write_str("invalid session token"),
            Self::EntropyUnavailable => {
                formatter.write_str("session entropy source is unavailable")
            }
            Self::InvalidClaims => formatter.write_str("session claims are invalid"),
            Self::ClaimsTooLarge { actual, maximum } => write!(
                formatter,
                "session claims reached {actual} bytes; maximum is {maximum}"
            ),
            Self::VersionMismatch => {
                formatter.write_str("session policy or schema version mismatch")
            }
            Self::StoreConflict => formatter.write_str("session store conflict"),
            Self::StoreFailure(message) => write!(formatter, "session store failed: {message}"),
            Self::Clock => formatter.write_str("session clock is unavailable"),
        }
    }
}

impl std::error::Error for SessionError {}
