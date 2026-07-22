// SPDX-License-Identifier: Apache-2.0

use pliego_pboc::{decode_manifest, verify_bundle};
use provider_tck::native::build_runtime_with_identity;
use std::path::PathBuf;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let root = PathBuf::from(std::env::var("PLIEGO_PBOC_ROOT")?);
    let bytes = std::fs::read(root.join(pliego_pboc::PBOC_FILE_NAME))?;
    let manifest = decode_manifest(&bytes)?;
    let verification = verify_bundle(&root, &manifest)?;
    let runtime =
        build_runtime_with_identity(&manifest.build.release_id, &verification.manifest_sha256)?;
    let admission = runtime.admit_pboc(&manifest, env!("CARGO_PKG_VERSION"))?;
    if admission.manifest_sha256 != verification.manifest_sha256 {
        return Err("PBOC admission and bundle verification differ".into());
    }
    let address: std::net::SocketAddr = std::env::var("PLIEGO_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:4330".to_owned())
        .parse()?;
    let listener = TcpListener::bind(address).await?;
    let bound = listener.local_addr()?;
    println!(
        "PLIEGO provider TCK native listening on http://{bound} pboc={}",
        admission.manifest_sha256
    );
    runtime
        .serve(listener, async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
