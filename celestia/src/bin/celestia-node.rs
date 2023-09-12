#[cfg(not(target_arch = "wasm32"))]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    celestia::run().await
}

// Placeholder to allow compilation
#[cfg(target_arch = "wasm32")]
fn main() {}
