use crate::{Collection, ContentId};
use std::collections::BTreeMap;

/// Versioned SHA-256 contract digest over ID, path, format, policy, and normalized source.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ContentFingerprint(String);

impl ContentFingerprint {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ContentFingerprint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Content identity state without the deserialized values.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CollectionSnapshot {
    entries: BTreeMap<ContentId, ContentFingerprint>,
}

impl CollectionSnapshot {
    pub(crate) fn from_collection<T>(collection: &Collection<T>) -> Self {
        Self {
            entries: collection
                .iter()
                .map(|entry| (entry.id().clone(), entry.fingerprint().clone()))
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&ContentFingerprint> {
        self.entries.get(id)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&ContentId, &ContentFingerprint)> {
        self.entries.iter()
    }

    /// Compare this snapshot with `next`.
    ///
    /// Added and changed IDs exist in `next`; removed IDs exist only in `self`.
    pub fn diff(&self, next: &Self) -> SnapshotDiff {
        let added = next
            .entries
            .keys()
            .filter(|id| !self.entries.contains_key(*id))
            .cloned()
            .collect();
        let removed = self
            .entries
            .keys()
            .filter(|id| !next.entries.contains_key(*id))
            .cloned()
            .collect();
        let changed = self
            .entries
            .iter()
            .filter_map(|(id, fingerprint)| {
                next.entries
                    .get(id)
                    .filter(|next_fingerprint| *next_fingerprint != fingerprint)
                    .map(|_| id.clone())
            })
            .collect();
        SnapshotDiff {
            added,
            changed,
            removed,
        }
    }
}

/// Portable, deterministically sorted changes between two snapshots.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SnapshotDiff {
    added: Vec<ContentId>,
    changed: Vec<ContentId>,
    removed: Vec<ContentId>,
}

impl SnapshotDiff {
    pub fn added(&self) -> &[ContentId] {
        &self.added
    }

    pub fn changed(&self) -> &[ContentId] {
        &self.changed
    }

    pub fn removed(&self) -> &[ContentId] {
        &self.removed
    }

    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.changed.is_empty() && self.removed.is_empty()
    }
}
