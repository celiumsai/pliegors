// SPDX-License-Identifier: Apache-2.0

wit_bindgen::generate!({
    path: "../../../crates/pliego-sdk/wit/build",
    world: "transformer",
});

use exports::pliego::build::transform::{Guest, TransformInput, TransformOutput};

struct UppercaseTransform;

impl Guest for UppercaseTransform {
    fn apply(input: TransformInput) -> Result<TransformOutput, String> {
        if input.bytes.len() > 64 * 1024 || input.options_json.len() > 4 * 1024 {
            return Err("transform input exceeds field limits".to_owned());
        }
        let source = std::str::from_utf8(&input.bytes)
            .map_err(|_| "uppercase-v1 accepts UTF-8 input only".to_owned())?;
        if !source.is_ascii() {
            return Err("uppercase-v1 accepts ASCII input only".to_owned());
        }
        let options: serde_json::Value = serde_json::from_str(&input.options_json)
            .map_err(|error| format!("invalid options JSON: {error}"))?;
        let object = options
            .as_object()
            .filter(|object| (1..=2).contains(&object.len()))
            .ok_or_else(|| "options must contain prefix and optional mode strings".to_owned())?;
        if object.keys().any(|key| key != "prefix" && key != "mode") {
            return Err("options contains an unknown field".to_owned());
        }
        let prefix = object
            .get("prefix")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "options prefix must be a string".to_owned())?;
        if prefix.len() > 1024 || !prefix.is_ascii() {
            return Err("prefix must be at most 1024 ASCII bytes".to_owned());
        }
        match object.get("mode").and_then(serde_json::Value::as_str) {
            None | Some("transform") => {}
            Some("spin") => loop {
                std::hint::black_box(());
            },
            Some(_) => return Err("unknown transform mode".to_owned()),
        }
        Ok(TransformOutput {
            media_type: "text/plain; charset=utf-8".to_owned(),
            bytes: format!("{prefix}{}", source.to_ascii_uppercase()).into_bytes(),
            diagnostics_json: "[]".to_owned(),
        })
    }
}

export!(UppercaseTransform);
