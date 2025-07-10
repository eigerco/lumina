use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use blockstore::EitherBlockstore;
use libp2p::identity::Keypair;
use lumina_node::blockstore::{InMemoryBlockstore, RedbBlockstore};
use lumina_node::network::Network;
use lumina_node::node::DEFAULT_PRUNING_WINDOW_IN_MEMORY;
use lumina_node::store::{EitherStore, InMemoryStore, RedbStore};
use lumina_node::NodeBuilder;
use tokio::task::spawn_blocking;
use uniffi::Record;

use crate::error::{LuminaError, Result};
use crate::{Blockstore, Store};

/// Configuration options for the Lumina node
#[derive(Debug, Clone, Record)]
pub struct NodeConfig {
    /// Base path for storing node data as a string. If this is not set then in-memory stores are used.
    pub base_path: Option<String>,
    /// Network to connect to
    pub network: Network,
    /// Custom list of bootstrap peers to connect to.
    /// If None, uses the canonical bootnodes for the network.
    pub bootnodes: Option<Vec<String>>,
    /// Custom sampling window in seconds. Default is 30 days.
    pub sampling_window_secs: Option<u32>,
    /// Custom pruning window in seconds. Default is 30 days plus 1 hour if base path is set
    /// and 0 seconds if not.
    pub pruning_window_secs: Option<u32>,
    /// Maximum number of headers in batch while syncing. Default is 128.
    pub batch_size: Option<u64>,
    /// Optional Set the keypair to be used as Node's identity. If None, generates a new Ed25519 keypair.
    pub ed25519_secret_key_bytes: Option<Vec<u8>>,
}

impl NodeConfig {
    /// Convert into NodeBuilder for the implementation
    pub(crate) async fn into_node_builder(self) -> Result<NodeBuilder<Blockstore, Store>> {
        let (blockstore, store) = match self.base_path {
            Some(ref base_path) => {
                let (blockstore, store) = open_persistent_stores(base_path, &self.network).await?;
                (
                    EitherBlockstore::Right(blockstore),
                    EitherStore::Right(store),
                )
            }
            None => (
                EitherBlockstore::Left(InMemoryBlockstore::new()),
                EitherStore::Left(InMemoryStore::new()),
            ),
        };

        let mut builder = NodeBuilder::new()
            .store(store)
            .blockstore(blockstore)
            .network(self.network)
            .sync_batch_size(self.batch_size.unwrap_or(128));

        // If base path is not set that means we use in-memory stores, so we
        // adjust pruning_window to avoid huge memory consumption.
        if self.base_path.is_none() {
            builder = builder.pruning_window(DEFAULT_PRUNING_WINDOW_IN_MEMORY);
        }

        if let Some(bootnodes) = self.bootnodes {
            let bootnodes = bootnodes
                .iter()
                .map(|addr| addr.parse())
                .collect::<Result<Vec<_>, _>>()?;
            builder = builder.bootnodes(bootnodes);
        }

        if let Some(key_bytes) = self.ed25519_secret_key_bytes {
            if key_bytes.len() != 32 {
                return Err(LuminaError::network("Ed25519 private key must be 32 bytes"));
            }

            let keypair = Keypair::ed25519_from_bytes(key_bytes)
                .map_err(|e| LuminaError::network(format!("Invalid Ed25519 key: {e}")))?;

            builder = builder.keypair(keypair);
        }

        if let Some(secs) = self.sampling_window_secs {
            builder = builder.sampling_window(Duration::from_secs(secs.into()));
        }

        if let Some(secs) = self.pruning_window_secs {
            builder = builder.pruning_window(Duration::from_secs(secs.into()));
        }

        Ok(builder)
    }
}

async fn open_persistent_stores(
    base_path: impl AsRef<Path>,
    network: &Network,
) -> Result<(RedbBlockstore, RedbStore)> {
    let base_path = base_path.as_ref().to_owned();
    let store_path = base_path.join(format!("store-{}", network.id()));

    let db = spawn_blocking(move || {
        std::fs::create_dir_all(&base_path)
            .map_err(|e| LuminaError::storage(format!("Failed to create base directory: {e}")))?;

        redb::Database::create(&store_path)
            .map(Arc::new)
            .map_err(|e| LuminaError::StorageInit {
                msg: format!("Failed to create database: {e}"),
            })
    })
    .await
    .map_err(|e| LuminaError::storage(format!("Failed to create base directory: {e}")))??;

    let store = RedbStore::new(db.clone())
        .await
        .map_err(|e| LuminaError::storage_init(format!("Failed to initialize store: {e}")))?;

    let blockstore = RedbBlockstore::new(db);

    Ok((blockstore, store))
}
