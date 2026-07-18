#![no_main]

use libfuzzer_sys::fuzz_target;
use pliego_fold::ProjectionSnapshot;
use sha2::{Digest, Sha256};

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn update_len_prefixed(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn synthetic_snapshot(input: &[u8]) -> Vec<u8> {
    let state = &input[..input.len().min(2_048)];
    let position = input.len() as u64;
    let head = digest(input);
    let schema = digest(&head);
    let reducer_id = b"fuzz/reducer";
    let codec_id = b"fuzz/json";
    let reducer_revision = 1_u64;
    let codec_revision = 1_u64;
    let reducer_config = digest(b"fuzz-reducer-config");
    let codec_config = digest(b"fuzz-codec-config");
    let state_digest = digest(state);

    let mut envelope_digest = Sha256::new();
    envelope_digest.update(b"pliego-fold/snapshot/1\0");
    envelope_digest.update(1_u16.to_be_bytes());
    envelope_digest.update(position.to_be_bytes());
    envelope_digest.update(head);
    envelope_digest.update(schema);
    update_len_prefixed(&mut envelope_digest, reducer_id);
    envelope_digest.update(reducer_revision.to_be_bytes());
    envelope_digest.update(reducer_config);
    update_len_prefixed(&mut envelope_digest, codec_id);
    envelope_digest.update(codec_revision.to_be_bytes());
    envelope_digest.update(codec_config);
    update_len_prefixed(&mut envelope_digest, state);
    envelope_digest.update(state_digest);
    let snapshot_digest: [u8; 32] = envelope_digest.finalize().into();

    let mut bytes = Vec::with_capacity(320 + state.len());
    bytes.extend_from_slice(b"PLGSNAP\0");
    bytes.extend_from_slice(&1_u16.to_be_bytes());
    bytes.extend_from_slice(&position.to_be_bytes());
    bytes.extend_from_slice(&head);
    bytes.extend_from_slice(&schema);
    bytes.extend_from_slice(&(reducer_id.len() as u16).to_be_bytes());
    bytes.extend_from_slice(reducer_id);
    bytes.extend_from_slice(&reducer_revision.to_be_bytes());
    bytes.extend_from_slice(&reducer_config);
    bytes.extend_from_slice(&(codec_id.len() as u16).to_be_bytes());
    bytes.extend_from_slice(codec_id);
    bytes.extend_from_slice(&codec_revision.to_be_bytes());
    bytes.extend_from_slice(&codec_config);
    bytes.extend_from_slice(&(state.len() as u32).to_be_bytes());
    bytes.extend_from_slice(state);
    bytes.extend_from_slice(&state_digest);
    bytes.extend_from_slice(&snapshot_digest);
    bytes
}

fuzz_target!(|data: &[u8]| {
    if data.len() > 64 * 1024 {
        return;
    }
    if let Ok(snapshot) = ProjectionSnapshot::decode(data) {
        let encoded = snapshot.encode();
        assert_eq!(ProjectionSnapshot::decode(&encoded).unwrap(), snapshot);
    }

    let valid = synthetic_snapshot(data);
    let decoded = ProjectionSnapshot::decode(&valid).expect("synthetic snapshot must be valid");
    assert_eq!(decoded.encode(), valid);
    if !data.is_empty() {
        let mut changed = valid;
        let index = usize::from(data[0]) % changed.len();
        changed[index] ^= 1;
        assert!(ProjectionSnapshot::decode(&changed).is_err());
    }
});
