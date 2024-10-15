#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;
use std::sync::Arc;

use libp2p::identity;
use lumina_node::{
    blockstore::RedbBlockstore,
    events::{EventSubscriber, NodeEvent},
    node::{Node, NodeConfig, DEFAULT_SYNCING_WINDOW},
    store::RedbStore,
};
use tempfile::tempdir;
use tokio::task::spawn_blocking;

use crate::utils::fetch_bridge_info;

mod utils;

#[tokio::test]
async fn graceful_stop() {
    let tmp_dir = tempdir().unwrap();

    let (node, mut events_sub) = new_node(tmp_dir.path()).await;
    let mut syncer_started = false;
    let mut daser_started = false;

    while !syncer_started || !daser_started {
        match events_sub.recv().await.unwrap().event {
            NodeEvent::FetchingHeadHeaderStarted
            | NodeEvent::FetchingHeadersStarted { .. }
            | NodeEvent::AddedHeaderFromHeaderSub { .. } => syncer_started = true,
            NodeEvent::SamplingStarted { .. } => daser_started = true,
            _ => {}
        }
    }

    // Initiate graceful stop
    node.stop().await;

    // We should be able to start a new node with the same database path
    let (_node, mut events_sub) = new_node(tmp_dir.path()).await;
    syncer_started = false;
    daser_started = false;

    while !syncer_started || !daser_started {
        match events_sub.recv().await.unwrap().event {
            NodeEvent::FetchingHeadHeaderStarted
            | NodeEvent::FetchingHeadersStarted { .. }
            | NodeEvent::AddedHeaderFromHeaderSub { .. } => syncer_started = true,
            NodeEvent::SamplingStarted { .. } => daser_started = true,
            _ => {}
        }
    }
}

async fn new_node(path: impl AsRef<Path>) -> (Node<RedbBlockstore, RedbStore>, EventSubscriber) {
    let path = path.as_ref().join("db");

    let db = spawn_blocking(|| Arc::new(redb::Database::create(path).unwrap()))
        .await
        .unwrap();

    let store = RedbStore::new(db.clone()).await.unwrap();
    let blockstore = RedbBlockstore::new(db);

    let (_, bridge_ma) = fetch_bridge_info().await;

    Node::new_subscribed(NodeConfig {
        network_id: "private".to_string(),
        p2p_local_keypair: identity::Keypair::generate_ed25519(),
        p2p_bootnodes: vec![bridge_ma],
        p2p_listen_on: vec![],
        sync_batch_size: 512,
        blockstore,
        store,
        syncing_window: DEFAULT_SYNCING_WINDOW,
    })
    .await
    .unwrap()
}
