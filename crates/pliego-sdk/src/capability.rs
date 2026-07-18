// SPDX-License-Identifier: Apache-2.0

use crate::Capability;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;

pub const MAX_EFFECT_RECEIPTS: usize = 4096;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CapabilityPolicy {
    grants: BTreeSet<Capability>,
}

impl CapabilityPolicy {
    pub const fn deny_all() -> Self {
        Self {
            grants: BTreeSet::new(),
        }
    }

    pub fn grant(mut self, capability: Capability) -> Self {
        self.grants.insert(capability);
        self
    }

    pub fn allows(&self, capability: Capability) -> bool {
        self.grants.contains(&capability)
    }

    pub fn require(
        &self,
        capability: Capability,
        operation: impl Into<String>,
    ) -> Result<(), CapabilityDenial> {
        if self.allows(capability) {
            Ok(())
        } else {
            Err(CapabilityDenial {
                capability,
                operation: operation.into(),
            })
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityDenial {
    pub capability: Capability,
    pub operation: String,
}

impl fmt::Display for CapabilityDenial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "capability `{}` denied for `{}`",
            self.capability.as_str(),
            self.operation
        )
    }
}

impl std::error::Error for CapabilityDenial {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EffectReceipt {
    pub schema: String,
    pub sequence: u64,
    pub outcome: EffectOutcome,
    pub capability: Capability,
    pub operation: String,
    pub input_sha256: String,
    pub output_sha256: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EffectOutcome {
    Success,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectFailure {
    pub message: String,
    pub receipt: EffectReceipt,
}

#[derive(Clone, Debug)]
pub struct EffectBroker {
    policy: CapabilityPolicy,
    next_sequence: u64,
    receipts: Vec<EffectReceipt>,
}

impl EffectBroker {
    pub fn new(policy: CapabilityPolicy) -> Self {
        Self {
            policy,
            next_sequence: 1,
            receipts: Vec::new(),
        }
    }

    pub fn execute<F>(
        &mut self,
        capability: Capability,
        operation: &str,
        input: &[u8],
        executor: F,
    ) -> Result<Vec<u8>, EffectError>
    where
        F: FnOnce() -> Result<Vec<u8>, String>,
    {
        validate_operation(operation)?;
        self.policy
            .require(Capability::EffectBroker, operation)
            .map_err(EffectError::Denied)?;
        self.policy
            .require(capability, operation)
            .map_err(EffectError::Denied)?;
        if self.receipts.len() >= MAX_EFFECT_RECEIPTS {
            return Err(EffectError::ReceiptLimitExceeded);
        }
        let next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(EffectError::SequenceExhausted)?;
        let result = executor();
        let (outcome, output_digest) = match &result {
            Ok(output) => (EffectOutcome::Success, Sha256::digest(output)),
            Err(error) => (EffectOutcome::Error, Sha256::digest(error.as_bytes())),
        };
        let receipt = EffectReceipt {
            schema: "dev.pliegors.effect-receipt/v1".to_owned(),
            sequence: self.next_sequence,
            outcome,
            capability,
            operation: operation.to_owned(),
            input_sha256: format!("sha256:{:x}", Sha256::digest(input)),
            output_sha256: format!("sha256:{output_digest:x}"),
        };
        self.next_sequence = next_sequence;
        self.receipts.push(receipt.clone());
        match result {
            Ok(output) => Ok(output),
            Err(message) => Err(EffectError::Executor(Box::new(EffectFailure {
                message,
                receipt,
            }))),
        }
    }

    pub fn receipts(&self) -> &[EffectReceipt] {
        &self.receipts
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EffectError {
    Denied(CapabilityDenial),
    InvalidOperation,
    Executor(Box<EffectFailure>),
    ReceiptLimitExceeded,
    SequenceExhausted,
}

impl fmt::Display for EffectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Denied(error) => error.fmt(formatter),
            Self::InvalidOperation => formatter.write_str("effect operation is invalid"),
            Self::Executor(error) => write!(formatter, "effect executor failed: {}", error.message),
            Self::ReceiptLimitExceeded => formatter.write_str("effect receipt limit exceeded"),
            Self::SequenceExhausted => formatter.write_str("effect receipt sequence exhausted"),
        }
    }
}

impl std::error::Error for EffectError {}

fn validate_operation(operation: &str) -> Result<(), EffectError> {
    if operation.is_empty()
        || operation.len() > 128
        || !operation.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(EffectError::InvalidOperation);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambient_effects_are_denied_without_executing() {
        let mut broker = EffectBroker::new(CapabilityPolicy::deny_all());
        let mut executed = false;
        let result = broker.execute(Capability::Network, "fetch", b"request", || {
            executed = true;
            Ok(b"response".to_vec())
        });
        assert!(matches!(result, Err(EffectError::Denied(_))));
        assert!(!executed);
        assert!(broker.receipts().is_empty());
    }

    #[test]
    fn granted_effects_are_digest_bound_and_ordered() {
        let policy = CapabilityPolicy::deny_all()
            .grant(Capability::EffectBroker)
            .grant(Capability::Network);
        let mut broker = EffectBroker::new(policy);
        assert_eq!(
            broker
                .execute(Capability::Network, "fetch", b"request", || Ok(
                    b"response".to_vec()
                ))
                .unwrap(),
            b"response"
        );
        assert_eq!(broker.receipts()[0].sequence, 1);
        assert_eq!(broker.receipts()[0].outcome, EffectOutcome::Success);
        assert_eq!(
            broker.receipts()[0].input_sha256,
            format!("sha256:{:x}", Sha256::digest(b"request"))
        );
    }

    #[test]
    fn attempted_failures_are_receipted_and_sequence_exhaustion_is_preflighted() {
        let policy = CapabilityPolicy::deny_all()
            .grant(Capability::EffectBroker)
            .grant(Capability::Network);
        let mut broker = EffectBroker::new(policy.clone());
        let error = broker
            .execute(Capability::Network, "fetch", b"request", || {
                Err("upstream unavailable".to_owned())
            })
            .unwrap_err();
        let EffectError::Executor(failure) = error else {
            panic!("executor failure was not preserved")
        };
        assert_eq!(failure.receipt.outcome, EffectOutcome::Error);
        assert_eq!(broker.receipts(), [failure.receipt]);

        let mut exhausted = EffectBroker {
            policy,
            next_sequence: u64::MAX,
            receipts: Vec::new(),
        };
        let mut executed = false;
        assert!(matches!(
            exhausted.execute(Capability::Network, "fetch", b"request", || {
                executed = true;
                Ok(Vec::new())
            }),
            Err(EffectError::SequenceExhausted)
        ));
        assert!(!executed);
        assert!(exhausted.receipts().is_empty());
    }

    #[test]
    fn invalid_operations_and_receipt_exhaustion_fail_before_execution() {
        let policy = CapabilityPolicy::deny_all()
            .grant(Capability::EffectBroker)
            .grant(Capability::Network);
        for operation in ["", "contains space", "line\nbreak"] {
            let mut broker = EffectBroker::new(policy.clone());
            let mut executed = false;
            assert!(matches!(
                broker.execute(Capability::Network, operation, b"request", || {
                    executed = true;
                    Ok(Vec::new())
                }),
                Err(EffectError::InvalidOperation)
            ));
            assert!(!executed);
        }

        let receipt = EffectReceipt {
            schema: "dev.pliegors.effect-receipt/v1".to_owned(),
            sequence: 1,
            outcome: EffectOutcome::Success,
            capability: Capability::Network,
            operation: "fetch".to_owned(),
            input_sha256: format!("sha256:{:x}", Sha256::digest([])),
            output_sha256: format!("sha256:{:x}", Sha256::digest([])),
        };
        let mut broker = EffectBroker {
            policy,
            next_sequence: (MAX_EFFECT_RECEIPTS as u64) + 1,
            receipts: vec![receipt; MAX_EFFECT_RECEIPTS],
        };
        let mut executed = false;
        assert!(matches!(
            broker.execute(Capability::Network, "fetch", b"request", || {
                executed = true;
                Ok(Vec::new())
            }),
            Err(EffectError::ReceiptLimitExceeded)
        ));
        assert!(!executed);
    }
}
