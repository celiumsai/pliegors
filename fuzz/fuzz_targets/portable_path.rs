#![no_main]

use libfuzzer_sys::fuzz_target;
use pliego_artifact::PortablePath;

fuzz_target!(|data: &[u8]| {
    if data.len() > 16 * 1024 {
        return;
    }
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(path) = PortablePath::parse(source) else {
        return;
    };
    let reparsed = PortablePath::parse(path.as_str()).expect("accepted path must round-trip");
    assert_eq!(reparsed, path);
    assert!(!path.as_str().starts_with('/'));
    assert!(!path.as_str().ends_with('/'));
    assert!(!path.as_str().contains(['\\', '\0']));
    assert!(
        !path
            .as_str()
            .split('/')
            .any(|part| matches!(part, "" | "." | ".."))
    );
});
