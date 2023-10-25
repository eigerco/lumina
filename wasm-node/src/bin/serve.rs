#[cfg(not(target_arch = "wasm32"))]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    wasm_node::server::run().await
}

// Placeholder to allow compilation
#[cfg(target_arch = "wasm32")]
fn main() {}
