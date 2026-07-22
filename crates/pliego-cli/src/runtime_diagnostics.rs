// SPDX-License-Identifier: Apache-2.0

use pliego_runtime::{CacheReceipt, InvalidationEvent, RuntimeContractManifest, RuntimeReceipt};
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

const DEFAULT_CONTRACT_PATH: &str = ".pliego/runtime-contract.json";
const MAX_DIAGNOSTIC_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_RECEIPT_ITEMS: usize = 1_024;

#[derive(Debug)]
pub(crate) struct DiagnosticCommandError {
    usage: bool,
    message: String,
}

impl DiagnosticCommandError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            usage: true,
            message: message.into(),
        }
    }

    fn artifact(message: impl Into<String>) -> Self {
        Self {
            usage: false,
            message: message.into(),
        }
    }

    pub(crate) fn is_usage(&self) -> bool {
        self.usage
    }
}

impl Display for DiagnosticCommandError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

pub(crate) fn why(arguments: &[String]) -> Result<String, DiagnosticCommandError> {
    if arguments.len() != 2 {
        return Err(DiagnosticCommandError::usage(
            "usage: pliego why <request|cache> <receipt.json>",
        ));
    }
    let bytes = read_bounded(Path::new(&arguments[1]))?;
    match arguments[0].as_str() {
        "request" => {
            let receipt: RuntimeReceipt = decode(&bytes, "runtime receipt")?;
            require_contract(
                &receipt.contract,
                "dev.pliegors.runtime-receipt/v1",
                "runtime receipt",
            )?;
            require_sha256(
                &receipt.application_contract_sha256,
                "runtime application contract",
            )?;
            validate_runtime_receipt(&receipt)?;
            Ok(receipt.explain())
        }
        "cache" => explain_cache(&bytes),
        _ => Err(DiagnosticCommandError::usage(
            "usage: pliego why <request|cache> <receipt.json>",
        )),
    }
}

pub(crate) fn inspect(arguments: &[String]) -> Result<String, DiagnosticCommandError> {
    if arguments.len() != 2 && arguments.len() != 4 {
        return Err(DiagnosticCommandError::usage(
            "usage: pliego inspect action <id> [--contract <runtime-contract.json>]",
        ));
    }
    if arguments[0] != "action" || (arguments.len() == 4 && arguments[2] != "--contract") {
        return Err(DiagnosticCommandError::usage(
            "usage: pliego inspect action <id> [--contract <runtime-contract.json>]",
        ));
    }
    let path = if arguments.len() == 4 {
        PathBuf::from(&arguments[3])
    } else {
        PathBuf::from(DEFAULT_CONTRACT_PATH)
    };
    let manifest: RuntimeContractManifest = decode(&read_bounded(&path)?, "runtime contract")?;
    require_contract(
        &manifest.contract,
        "dev.pliegors.runtime-contract/v1",
        "runtime contract",
    )?;
    require_sha256(
        &manifest.application_contract_sha256,
        "runtime application contract",
    )?;
    validate_runtime_contract(&manifest)?;
    let action = manifest
        .actions
        .iter()
        .find(|action| action.id == arguments[1])
        .ok_or_else(|| {
            DiagnosticCommandError::artifact(format!(
                "action `{}` is absent from {}",
                arguments[1],
                path.display()
            ))
        })?;
    require_sha256(&action.contract_sha256, "action contract")?;
    Ok(format!(
        "application contract: {}\n{}",
        manifest.application_contract_sha256,
        action.explain()
    ))
}

fn explain_cache(bytes: &[u8]) -> Result<String, DiagnosticCommandError> {
    if let Ok(receipt) = serde_json::from_slice::<CacheReceipt>(bytes) {
        require_contract(
            &receipt.contract,
            "dev.pliegors.cache-receipt/v1",
            "cache receipt",
        )?;
        require_sha256(&receipt.key_digest, "cache key")?;
        validate_cache_receipt(&receipt)?;
        return Ok(receipt.explain());
    }
    let event: InvalidationEvent = decode(bytes, "cache receipt or invalidation event")?;
    require_contract(
        &event.contract,
        "dev.pliegors.cache-invalidation/v1",
        "cache invalidation event",
    )?;
    validate_invalidation_event(&event)?;
    event.require_acknowledgements().map_err(|error| {
        DiagnosticCommandError::artifact(format!(
            "cache invalidation acknowledgement barrier failed: {error}"
        ))
    })?;
    Ok(event.explain())
}

fn validate_runtime_receipt(receipt: &RuntimeReceipt) -> Result<(), DiagnosticCommandError> {
    require_runtime_identity(&receipt.request_id, "request ID")?;
    require_runtime_identity(&receipt.deployment_id, "deployment ID")?;
    require_sha256(&receipt.limit_policy_sha256, "request limit policy")?;
    if let Some(route) = &receipt.route_id {
        require_stable_id(route, "route ID")?;
    }
    for (values, kind) in [
        (&receipt.route_scopes, "route scope"),
        (&receipt.route_layouts, "route layout"),
        (&receipt.middleware, "middleware"),
    ] {
        require_item_bound(values.len(), kind)?;
        for value in values {
            require_stable_id(value, kind)?;
        }
    }
    if let Some(boundary) = &receipt.error_boundary {
        require_stable_id(boundary, "error boundary")?;
    }
    require_item_bound(receipt.diagnostics.len(), "runtime diagnostics")?;
    for diagnostic in &receipt.diagnostics {
        require_diagnostic_code(&diagnostic.code)?;
    }
    require_item_bound(receipt.data_receipts.len(), "data receipts")?;
    for data in &receipt.data_receipts {
        require_contract(
            &data.contract,
            "dev.pliegors.data-receipt/v1",
            "data receipt",
        )?;
        require_stable_id(&data.operation_id, "data operation ID")?;
        if data.semantic_revision == 0 {
            return Err(DiagnosticCommandError::artifact(
                "data receipt semantic revision must be non-zero",
            ));
        }
        if let Some(code) = &data.diagnostic_code {
            require_diagnostic_code(code)?;
        }
    }
    require_item_bound(receipt.cache_receipts.len(), "cache receipts")?;
    for cache in &receipt.cache_receipts {
        validate_cache_receipt(cache)?;
    }
    require_item_bound(receipt.invalidation_events.len(), "invalidation events")?;
    for event in &receipt.invalidation_events {
        validate_invalidation_event(event)?;
    }
    Ok(())
}

fn validate_cache_receipt(receipt: &CacheReceipt) -> Result<(), DiagnosticCommandError> {
    require_contract(
        &receipt.contract,
        "dev.pliegors.cache-receipt/v1",
        "cache receipt",
    )?;
    require_stable_id(&receipt.policy_id, "cache policy ID")?;
    require_stable_id(&receipt.namespace, "cache namespace")?;
    require_sha256(&receipt.key_digest, "cache key")?;
    if receipt.semantic_revision == 0 || receipt.compatibility_epoch == 0 {
        return Err(DiagnosticCommandError::artifact(
            "cache receipt revisions must be non-zero",
        ));
    }
    Ok(())
}

fn validate_invalidation_event(event: &InvalidationEvent) -> Result<(), DiagnosticCommandError> {
    require_contract(
        &event.contract,
        "dev.pliegors.cache-invalidation/v1",
        "cache invalidation event",
    )?;
    require_stable_id(&event.policy_id, "cache policy ID")?;
    require_stable_id(&event.namespace, "cache namespace")?;
    require_stable_id(&event.cause_receipt, "invalidation cause")?;
    require_sha256(&event.target_digest, "cache invalidation target")?;
    if event.compatibility_epoch == 0 || event.sequence == 0 {
        return Err(DiagnosticCommandError::artifact(
            "cache invalidation sequence and epoch must be non-zero",
        ));
    }
    Ok(())
}

fn validate_runtime_contract(
    manifest: &RuntimeContractManifest,
) -> Result<(), DiagnosticCommandError> {
    require_item_bound(manifest.actions.len(), "actions")?;
    require_item_bound(manifest.loaders.len(), "loaders")?;
    require_item_bound(manifest.caches.len(), "cache policies")?;
    for action in &manifest.actions {
        require_stable_id(&action.id, "action ID")?;
        require_sha256(&action.contract_sha256, "action contract")?;
        if action.semantic_revision == 0 {
            return Err(DiagnosticCommandError::artifact(
                "action semantic revision must be non-zero",
            ));
        }
        require_item_bound(action.accepted_media_types.len(), "action media types")?;
        for value in &action.accepted_media_types {
            require_enum(
                value,
                &[
                    "application/x-www-form-urlencoded",
                    "application/json",
                    "multipart/form-data",
                ],
                "action media type",
            )?;
        }
        require_item_bound(
            action.accepted_content_encodings.len(),
            "action content encodings",
        )?;
        for value in &action.accepted_content_encodings {
            require_enum(value, &["identity", "gzip"], "action content encoding")?;
        }
        require_enum(
            &action.origin_policy,
            &["same-origin", "explicitly-non-browser"],
            "action Origin policy",
        )?;
        require_enum(
            &action.csrf_policy,
            &["same-origin", "session-bound-token"],
            "action CSRF policy",
        )?;
        if let Some(id) = &action.idempotency_policy_id {
            require_stable_id(id, "idempotency policy ID")?;
        }
        require_item_bound(action.resources.len(), "action resources")?;
        for resource in &action.resources {
            require_stable_id(&resource.id, "resource ID")?;
            require_item_bound(resource.capabilities.len(), "resource capabilities")?;
            for capability in &resource.capabilities {
                require_stable_id(capability, "resource capability")?;
            }
        }
        require_item_bound(action.invalidations.len(), "action invalidations")?;
        for invalidation in &action.invalidations {
            require_stable_id(&invalidation.cache_policy_id, "cache policy ID")?;
            require_enum(
                &invalidation.consistency,
                &["eventual", "read-your-writes"],
                "invalidation consistency",
            )?;
            require_item_bound(invalidation.tags.len(), "invalidation tags")?;
            for tag in &invalidation.tags {
                require_stable_id(tag, "cache tag")?;
            }
        }
    }
    for loader in &manifest.loaders {
        require_stable_id(&loader.id, "loader ID")?;
        require_sha256(&loader.contract_sha256, "loader contract")?;
        if loader.semantic_revision == 0 {
            return Err(DiagnosticCommandError::artifact(
                "loader semantic revision must be non-zero",
            ));
        }
        if let Some(id) = &loader.cache_policy_id {
            require_stable_id(id, "cache policy ID")?;
        }
        require_item_bound(loader.resources.len(), "loader resources")?;
        for resource in &loader.resources {
            require_stable_id(&resource.id, "resource ID")?;
            for capability in &resource.capabilities {
                require_stable_id(capability, "resource capability")?;
            }
        }
    }
    for cache in &manifest.caches {
        require_stable_id(&cache.id, "cache policy ID")?;
        require_stable_id(&cache.namespace, "cache namespace")?;
        require_sha256(&cache.contract_sha256, "cache contract")?;
        require_enum(
            &cache.domain,
            &["public-runtime", "private-request", "private-session"],
            "cache domain",
        )?;
        if cache.semantic_revision == 0 || cache.compatibility_epoch == 0 {
            return Err(DiagnosticCommandError::artifact(
                "cache contract revisions must be non-zero",
            ));
        }
    }
    Ok(())
}

fn require_item_bound(actual: usize, kind: &str) -> Result<(), DiagnosticCommandError> {
    if actual <= MAX_RECEIPT_ITEMS {
        Ok(())
    } else {
        Err(DiagnosticCommandError::artifact(format!(
            "{kind} exceed the diagnostic item bound"
        )))
    }
}

fn require_runtime_identity(value: &str, kind: &str) -> Result<(), DiagnosticCommandError> {
    if !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        Ok(())
    } else {
        Err(DiagnosticCommandError::artifact(format!(
            "{kind} is malformed"
        )))
    }
}

fn require_stable_id(value: &str, kind: &str) -> Result<(), DiagnosticCommandError> {
    let mut bytes = value.bytes();
    let valid = value.len() <= 96
        && bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.ends_with('-')
        && !value.contains("--");
    if valid {
        Ok(())
    } else {
        Err(DiagnosticCommandError::artifact(format!(
            "{kind} is malformed"
        )))
    }
}

fn require_diagnostic_code(value: &str) -> Result<(), DiagnosticCommandError> {
    if !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
    {
        Ok(())
    } else {
        Err(DiagnosticCommandError::artifact(
            "runtime diagnostic code is malformed",
        ))
    }
}

fn require_enum(value: &str, allowed: &[&str], kind: &str) -> Result<(), DiagnosticCommandError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(DiagnosticCommandError::artifact(format!(
            "{kind} `{value}` is unsupported"
        )))
    }
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, DiagnosticCommandError> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        DiagnosticCommandError::artifact(format!("cannot read {}: {error}", path.display()))
    })?;
    if !metadata.is_file() || metadata.len() > MAX_DIAGNOSTIC_FILE_BYTES {
        return Err(DiagnosticCommandError::artifact(format!(
            "diagnostic input {} must be a file no larger than {} bytes",
            path.display(),
            MAX_DIAGNOSTIC_FILE_BYTES
        )));
    }
    std::fs::read(path).map_err(|error| {
        DiagnosticCommandError::artifact(format!("cannot read {}: {error}", path.display()))
    })
}

fn decode<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
    kind: &str,
) -> Result<T, DiagnosticCommandError> {
    serde_json::from_slice(bytes).map_err(|_| {
        DiagnosticCommandError::artifact(format!(
            "input is not a canonical supported {kind} JSON document"
        ))
    })
}

fn require_contract(
    actual: &str,
    expected: &str,
    kind: &str,
) -> Result<(), DiagnosticCommandError> {
    if actual == expected {
        Ok(())
    } else {
        Err(DiagnosticCommandError::artifact(format!(
            "unsupported {kind} contract `{actual}`; expected `{expected}`"
        )))
    }
}

fn require_sha256(value: &str, kind: &str) -> Result<(), DiagnosticCommandError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(DiagnosticCommandError::artifact(format!(
            "{kind} SHA-256 is malformed"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pliego_runtime::{
        ActionContractManifest, ActionInvalidationManifest, CacheDomain, CacheOutcome,
        CacheSizeBucket, ContractResourceRequirement, RequestDurationBucket, RequestOutcome,
        RequestState,
    };

    fn write_json(name: &str, value: &impl serde::Serialize) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "pliego-g2-{name}-{}-{nonce}.json",
            std::process::id(),
        ));
        std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    }

    #[test]
    fn cache_receipt_explanation_is_bounded_and_redacted() {
        let receipt = CacheReceipt {
            contract: "dev.pliegors.cache-receipt/v1".to_owned(),
            policy_id: "catalog-public".to_owned(),
            semantic_revision: 1,
            namespace: "catalog".to_owned(),
            compatibility_epoch: 1,
            key_digest: "a".repeat(64),
            domain: CacheDomain::PublicRuntime,
            outcome: CacheOutcome::Hit,
            value_size_bucket: CacheSizeBucket::Under1Kibibyte,
            invalidation_sequence: None,
        };
        let path = write_json("cache", &receipt);
        let output = why(&["cache".to_owned(), path.display().to_string()]).unwrap();
        let _ = std::fs::remove_file(path);
        assert!(output.contains("PLIEGO why cache"));
        assert!(output.contains("outcome: hit"));
        assert!(!output.contains("private-value"));
    }

    #[test]
    fn request_receipt_explanation_validates_the_contract_and_stays_redacted() {
        let receipt = RuntimeReceipt {
            contract: "dev.pliegors.runtime-receipt/v1".to_owned(),
            application_contract_sha256: "d".repeat(64),
            request_id: "request-0001".to_owned(),
            deployment_id: "deployment-preview".to_owned(),
            route_id: Some("dashboard".to_owned()),
            route_scopes: vec!["account".to_owned()],
            route_layouts: Vec::new(),
            limit_policy_sha256: "e".repeat(64),
            outcome: RequestOutcome::Success,
            final_state: RequestState::Closed,
            cancel_reason: None,
            response_status: Some(200),
            response_bytes: 512,
            duration_bucket: RequestDurationBucket::Under10Milliseconds,
            render_mode: None,
            middleware: vec!["security".to_owned()],
            error_boundary: None,
            diagnostics: Vec::new(),
            data_receipts: Vec::new(),
            cache_receipts: Vec::new(),
            invalidation_events: Vec::new(),
        };
        let path = write_json("request", &receipt);
        let output = why(&["request".to_owned(), path.display().to_string()]).unwrap();
        let _ = std::fs::remove_file(path);
        assert!(output.contains("PLIEGO why request request-0001"));
        assert!(output.contains("route: dashboard"));
        assert!(!output.contains("password"));

        let mut hostile = receipt;
        hostile.request_id = "request\u{1b}[31m".to_owned();
        let path = write_json("hostile-request", &hostile);
        let error = why(&["request".to_owned(), path.display().to_string()]).unwrap_err();
        let _ = std::fs::remove_file(path);
        assert!(error.to_string().contains("request ID is malformed"));
    }

    #[test]
    fn action_inspection_reads_only_the_versioned_contract_projection() {
        let manifest = RuntimeContractManifest {
            contract: "dev.pliegors.runtime-contract/v1".to_owned(),
            application_contract_sha256: "b".repeat(64),
            actions: vec![ActionContractManifest {
                id: "rename-account".to_owned(),
                semantic_revision: 1,
                contract_sha256: "c".repeat(64),
                accepted_media_types: vec!["application/x-www-form-urlencoded".to_owned()],
                accepted_content_encodings: vec!["identity".to_owned()],
                max_encoded_bytes: 1024,
                max_decoded_bytes: 1024,
                max_form_fields: 16,
                max_output_bytes: 1024,
                origin_policy: "same-origin".to_owned(),
                csrf_policy: "session-bound-token".to_owned(),
                requires_authentication: true,
                requires_authorization: true,
                post_commit_grace_ms: 2_000,
                idempotency_policy_id: Some("account-mutations".to_owned()),
                resources: vec![ContractResourceRequirement {
                    id: "accounts".to_owned(),
                    capabilities: vec!["write".to_owned()],
                }],
                invalidations: vec![ActionInvalidationManifest {
                    cache_policy_id: "account-private".to_owned(),
                    tags: vec!["account-private".to_owned()],
                    consistency: "read-your-writes".to_owned(),
                }],
            }],
            loaders: Vec::new(),
            caches: Vec::new(),
        };
        let path = write_json("contract", &manifest);
        let output = inspect(&[
            "action".to_owned(),
            "rename-account".to_owned(),
            "--contract".to_owned(),
            path.display().to_string(),
        ])
        .unwrap();
        let _ = std::fs::remove_file(path);
        assert!(output.contains("PLIEGO inspect action rename-account"));
        assert!(output.contains("read-your-writes"));
        assert!(!output.contains("private-value"));

        let mut hostile = manifest;
        hostile.actions[0].origin_policy = "same-origin\u{1b}[31m".to_owned();
        let path = write_json("hostile-contract", &hostile);
        let error = inspect(&[
            "action".to_owned(),
            "rename-account".to_owned(),
            "--contract".to_owned(),
            path.display().to_string(),
        ])
        .unwrap_err();
        let _ = std::fs::remove_file(path);
        assert!(error.to_string().contains("Origin policy"));
    }
}
