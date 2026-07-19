// SPDX-License-Identifier: Apache-2.0

use native_pliego::{build_runtime, configured_address};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let address = configured_address()?;
    let listener = TcpListener::bind(address).await?;
    let bound = listener.local_addr()?;
    let runtime = build_runtime()?;
    println!("Native PliegoRS preview listening on http://{bound}");
    runtime
        .serve(listener, async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
