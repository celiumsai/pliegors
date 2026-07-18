#![no_main]

use libfuzzer_sys::fuzz_target;
use pliego_cli::project_manifest::parse_project_manifest;

fn valid_shaped_source(data: &[u8]) -> String {
    let id_tail = data
        .iter()
        .take(40)
        .map(|byte| match byte % 37 {
            0..=25 => char::from(b'a' + (byte % 26)),
            26..=35 => char::from(b'0' + (byte % 10)),
            _ => '-',
        })
        .collect::<String>();
    let output = match data.first().copied().unwrap_or_default() % 5 {
        0 => "target/site",
        1 => "target/site/nested",
        2 => "target/.pliego",
        3 => "../outside",
        _ => "target/site/../outside",
    };
    let client = if data.get(1).copied().unwrap_or_default() & 1 == 0 {
        String::new()
    } else {
        format!(
            "\n[client]\npackage = \"sample-client\"\nwasm_name = \"sample_client\"\nbindgen_output = \"{}\"\n",
            if data.get(2).copied().unwrap_or_default() & 1 == 0 {
                "target/wasm"
            } else {
                output
            }
        )
    };
    format!(
        "[project]\nid = \"a{id_tail}\"\nname = \"Sample\"\nsite_package = \"sample-site\"\noutput = \"{output}\"\n{client}"
    )
}

fuzz_target!(|data: &[u8]| {
    if data.len() > 64 * 1024 {
        return;
    }
    if let Ok(source) = std::str::from_utf8(data)
        && let Ok(first) = parse_project_manifest(source)
    {
        assert_eq!(parse_project_manifest(source).unwrap(), first);
    }
    let shaped = valid_shaped_source(data);
    if let Ok(first) = parse_project_manifest(&shaped) {
        assert_eq!(parse_project_manifest(&shaped).unwrap(), first);
    }
});
