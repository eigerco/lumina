use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    celestia::run().await
}
