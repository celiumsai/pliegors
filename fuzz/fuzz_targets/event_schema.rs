#![no_main]

use libfuzzer_sys::fuzz_target;
use pliego_log::CanonicalJson;

fuzz_target!(|data: &[u8]| {
    if data.len() > 64 * 1024 {
        return;
    }
    let Ok(canonical) = CanonicalJson::parse(data) else {
        return;
    };
    let reparsed = CanonicalJson::parse(canonical.as_bytes())
        .expect("canonical event payload must remain admissible");
    assert_eq!(reparsed, canonical);
    let value: serde_json::Value =
        serde_json::from_slice(canonical.as_bytes()).expect("canonical JSON must decode");
    assert_eq!(CanonicalJson::from_serialize(&value).unwrap(), canonical);
});
