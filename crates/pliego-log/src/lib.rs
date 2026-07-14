// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Celiums Solutions LLC

//! pliego-log — the append-only, hash-chained event log at the root of PliegoRS.
//!
//! This is the intended root for application-domain writes (docs/00 §1/§3):
//! interactions append events here and projected state folds the log. Runtime
//! signals remain writable for framework and UI coordination. The chain
//! (`hash = H(prev_hash ‖ seq ‖ kind ‖ payload)`) follows the same broad
//! discipline as Hyphae's journal, but the client and durable wire contracts are
//! not identical yet (roadmap M5).
//!
//! Deliberately deterministic: no timestamps or randomness inside the hash — the
//! same events always produce the same local chain. This supports replay; SSR
//! hydration still requires its own serialized-prefix and DOM-adoption protocol.

use sha2::{Digest, Sha256};

/// A 32-byte SHA-256 hash.
pub type Hash = [u8; 32];

/// One immutable event in the log. `seq` doubles as a stable identity for
/// anything the event creates. That correlation is a starting point for the
/// richer provenance model planned after M5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// 0-based position in the log.
    pub seq: u64,
    /// Discriminant, e.g. `"task_added"`. Reducers match on this.
    pub kind: String,
    /// Opaque payload (UTF-8 by convention; the log doesn't interpret it).
    pub payload: String,
    /// Hash of the previous event (all zeroes for the genesis event).
    pub prev_hash: Hash,
    /// This event's hash: `H(prev_hash ‖ seq ‖ kind ‖ payload)`.
    pub hash: Hash,
}

/// Where a chain verification failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TamperedAt(pub u64);

/// The append-only log. In-memory for M1; durability (snapshot + persist) is
/// additive and does not change this API.
#[derive(Debug, Default, Clone)]
pub struct Log {
    events: Vec<Event>,
}

impl Log {
    /// An empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of events.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.events.len() as u64
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The head hash (all zeroes when empty) — what the next event chains from.
    #[must_use]
    pub fn head(&self) -> Hash {
        self.events.last().map_or([0u8; 32], |e| e.hash)
    }

    /// Append an event. The ONLY write in the entire framework.
    /// Returns the stored event (with its seq and hash).
    pub fn append(&mut self, kind: impl Into<String>, payload: impl Into<String>) -> &Event {
        let kind = kind.into();
        let payload = payload.into();
        let seq = self.len();
        let prev_hash = self.head();
        let hash = event_hash(&prev_hash, seq, &kind, &payload);
        self.events.push(Event {
            seq,
            kind,
            payload,
            prev_hash,
            hash,
        });
        self.events.last().expect("just pushed")
    }

    /// All events (a fold from genesis reads this).
    #[must_use]
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// The events at and after `cursor` — the tail an incremental fold consumes.
    #[must_use]
    pub fn tail(&self, cursor: u64) -> &[Event] {
        let start = (cursor as usize).min(self.events.len());
        &self.events[start..]
    }

    /// Read one event by sequence number.
    #[must_use]
    pub fn get(&self, seq: u64) -> Option<&Event> {
        self.events.get(seq as usize)
    }

    /// Verify the whole chain: every event's `prev_hash` links its predecessor
    /// and its `hash` recomputes. `Err(TamperedAt(seq))` localizes the break.
    pub fn verify(&self) -> Result<(), TamperedAt> {
        let mut prev = [0u8; 32];
        for e in &self.events {
            if e.prev_hash != prev || e.hash != event_hash(&prev, e.seq, &e.kind, &e.payload) {
                return Err(TamperedAt(e.seq));
            }
            prev = e.hash;
        }
        Ok(())
    }
}

/// `H(prev_hash ‖ seq_le ‖ kind_len_le ‖ kind ‖ payload)`. The length prefix on
/// `kind` prevents ambiguity between (kind, payload) splits.
fn event_hash(prev: &Hash, seq: u64, kind: &str, payload: &str) -> Hash {
    let mut h = Sha256::new();
    h.update(prev);
    h.update(seq.to_le_bytes());
    h.update((kind.len() as u64).to_le_bytes());
    h.update(kind.as_bytes());
    h.update(payload.as_bytes());
    h.finalize().into()
}

/// Render a hash as lowercase hex (for provenance display).
#[must_use]
pub fn hex(hash: &Hash) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(hash.len() * 2);
    for byte in hash {
        encoded.push(DIGITS[(byte >> 4) as usize] as char);
        encoded.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    encoded
}

// ───────────────────────── the fold (hand-rolled for M1) ─────────────────────────

/// An incremental fold of the log: `state = reduce(state, events[cursor..])`.
///
/// This is the M1 hand-rolled version of THE new primitive (docs/00 §3.1): a
/// memo whose recompute is incremental — an accumulator plus a cursor into the
/// log, so syncing after an append costs O(new events), never O(log). In M3 this
/// becomes a first-class reactive node; the semantics here are the contract.
///
/// The reducer must be **pure and deterministic** (docs/00 §3.3): replaying the
/// same prefix always rebuilds the same state — that is what the M1 gate proves.
pub struct Fold<S> {
    state: S,
    cursor: u64,
    reducer: fn(&mut S, &Event),
}

impl<S> Fold<S> {
    /// A fold with an initial state and a pure reducer.
    pub fn new(initial: S, reducer: fn(&mut S, &Event)) -> Self {
        Self {
            state: initial,
            cursor: 0,
            reducer,
        }
    }

    /// Consume the log's tail since our cursor. Returns how many events were
    /// folded (0 = already in sync). O(tail) by construction.
    pub fn sync(&mut self, log: &Log) -> u64 {
        let tail = log.tail(self.cursor);
        for e in tail {
            (self.reducer)(&mut self.state, e);
        }
        let n = tail.len() as u64;
        self.cursor += n;
        n
    }

    /// The folded state.
    pub fn state(&self) -> &S {
        &self.state
    }

    /// How far into the log this fold has read.
    pub fn cursor(&self) -> u64 {
        self.cursor
    }
}

impl<S: Default> Fold<S> {
    /// Rebuild state from genesis up to (excluding) `upto` — time-travel/undo:
    /// "the world as of event N". Pure function of the log prefix.
    #[must_use]
    pub fn replay(log: &Log, upto: u64, reducer: fn(&mut S, &Event)) -> S {
        let mut state = S::default();
        for e in log.events().iter().take(upto as usize) {
            reducer(&mut state, e);
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default, Clone, PartialEq, Eq)]
    struct Counter {
        value: i64,
        events_seen: u64,
    }

    fn count(state: &mut Counter, e: &Event) {
        state.events_seen += 1;
        match e.kind.as_str() {
            "inc" => state.value += 1,
            "dec" => state.value -= 1,
            _ => {}
        }
    }

    #[test]
    fn chain_links_and_verifies() {
        let mut log = Log::new();
        log.append("inc", "");
        log.append("inc", "");
        log.append("dec", "");
        assert_eq!(log.len(), 3);
        assert!(log.verify().is_ok());
        // the chain actually links
        assert_eq!(log.get(1).unwrap().prev_hash, log.get(0).unwrap().hash);
    }

    #[test]
    fn tampering_is_localized() {
        let mut log = Log::new();
        log.append("inc", "a");
        log.append("inc", "b");
        log.append("inc", "c");
        // mutate event 1's payload behind the log's back
        log.events[1].payload = "TAMPERED".into();
        assert_eq!(log.verify(), Err(TamperedAt(1)));
    }

    #[test]
    fn determinism_same_events_same_chain() {
        let build = || {
            let mut l = Log::new();
            l.append("inc", "x");
            l.append("dec", "y");
            l.head()
        };
        assert_eq!(build(), build());
    }

    /// THE M1 GATE, part 1: live incremental state == replay from genesis,
    /// bit for bit, at every point in the log's growth.
    #[test]
    fn gate_replay_equals_live_state() {
        let mut log = Log::new();
        let mut live = Fold::new(Counter::default(), count);
        for i in 0..500 {
            log.append(if i % 3 == 0 { "dec" } else { "inc" }, format!("{i}"));
            live.sync(&log);
            let replayed = Fold::replay(&log, log.len(), count);
            assert_eq!(live.state(), &replayed, "diverged at event {i}");
        }
        assert!(log.verify().is_ok());
    }

    /// THE M1 GATE, part 2: append is O(tail) — syncing after one append folds
    /// exactly one event, regardless of how long the log already is.
    #[test]
    fn gate_sync_is_o_tail() {
        let mut log = Log::new();
        let mut live = Fold::new(Counter::default(), count);
        for _ in 0..1000 {
            log.append("inc", "");
        }
        assert_eq!(live.sync(&log), 1000); // first sync folds the backlog
        log.append("inc", "");
        assert_eq!(live.sync(&log), 1); // ...then exactly one per append
        assert_eq!(live.sync(&log), 0); // ...and zero when in sync
        // the fold saw each event exactly once (no re-folding from genesis)
        assert_eq!(live.state().events_seen, 1001);
    }

    #[test]
    fn time_travel_is_a_prefix_replay() {
        let mut log = Log::new();
        for _ in 0..10 {
            log.append("inc", "");
        }
        let at5: Counter = Fold::replay(&log, 5, count);
        assert_eq!(at5.value, 5);
        let at0: Counter = Fold::replay(&log, 0, count);
        assert_eq!(at0, Counter::default());
    }
}
