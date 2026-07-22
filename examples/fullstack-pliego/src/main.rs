// SPDX-License-Identifier: Apache-2.0

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut arguments = std::env::args().skip(1);
    if arguments.next().as_deref() == Some("contract") {
        let path = arguments
            .next()
            .unwrap_or_else(|| ".pliego/runtime-contract.json".to_owned());
        if arguments.next().is_some() {
            return Err("usage: fullstack-pliego contract [output.json]".into());
        }
        let cluster = fullstack_pliego::build_cluster()?;
        cluster.write_contract_manifest(std::path::Path::new(&path))?;
        println!("PLIEGO runtime contract: {path}");
        return Ok(());
    }
    let address = std::env::var("PLIEGO_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:4320".to_owned())
        .parse()?;
    fullstack_pliego::serve_single(address).await
}
