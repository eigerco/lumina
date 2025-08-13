# Lumina node

A crate to configure, run and interact with Celestia's data availability nodes.

```rust,no_run,ignore-wasm32
use std::sync::Arc;

use lumina_node::blockstore::RedbBlockstore;
use lumina_node::network::Network;
use lumina_node::node::Node;
use lumina_node::store::RedbStore;
use redb::Database;
use tokio::task::spawn_blocking;

#[tokio::main]
async fn main() {
    let db = spawn_blocking(|| Database::create("lumina.redb"))
        .await
        .expect("Failed to join")
        .expect("Failed to open the database");
    let db = Arc::new(db);

    let store = RedbStore::new(db.clone())
        .await
        .expect("Failed to create a store");
    let blockstore = RedbBlockstore::new(db);

    let node = Node::builder()
        .store(store)
        .blockstore(blockstore)
        .network(Network::Mainnet)
        .listen(["/ip4/0.0.0.0/tcp/0".parse().unwrap()])
        .start()
        .await
        .expect("Failed to build and start node");

    node.wait_connected().await.expect("Failed to connect");

    let header = node
        .request_header_by_height(15)
        .await
        .expect("Height not found");

    println!("{}", serde_json::to_string_pretty(&header).unwrap());
}
```
