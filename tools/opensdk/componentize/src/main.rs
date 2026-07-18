// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs;
use wit_component::ComponentEncoder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os().skip(1);
    let input = arguments.next().ok_or("missing core wasm input path")?;
    let output = arguments.next().ok_or("missing component output path")?;
    if arguments.next().is_some() {
        return Err("usage: pliego-opensdk-componentize <core.wasm> <component.wasm>".into());
    }
    let core = fs::read(&input)?;
    let component = ComponentEncoder::default()
        .module(&core)?
        .validate(true)
        .encode()?;
    if let Some(parent) = std::path::Path::new(&output).parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, component)?;
    Ok(())
}
