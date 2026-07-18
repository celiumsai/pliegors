// SPDX-License-Identifier: Apache-2.0

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::io::{self, Read};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    source: String,
    prefix: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Output {
    schema: &'static str,
    media_type: &'static str,
    bytes_base64: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut encoded = String::new();
    io::stdin()
        .take(1024 * 1024 + 1)
        .read_to_string(&mut encoded)?;
    if encoded.len() > 1024 * 1024 {
        return Err("input exceeds 1 MiB".into());
    }
    let input: Input = serde_json::from_str(&encoded)?;
    validate(&input)?;
    let transformed = format!("{}{}", input.prefix, input.source.to_ascii_uppercase());
    let output = Output {
        schema: "dev.pliegors.build-transform/v1",
        media_type: "text/plain; charset=utf-8",
        bytes_base64: STANDARD.encode(transformed.as_bytes()),
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

fn validate(input: &Input) -> Result<(), Box<dyn std::error::Error>> {
    if input.source.len() > 64 * 1024 || input.prefix.len() > 1024 {
        return Err("transform input exceeds field limits".into());
    }
    if !input.source.is_ascii() || !input.prefix.is_ascii() {
        return Err("uppercase-v1 accepts ASCII input only".into());
    }
    Ok(())
}
