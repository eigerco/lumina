use std::{path::PathBuf, sync::Arc, time::Duration};

use libp2p::identity::Keypair;
use lumina_node::{blockstore::RedbBlockstore, network, store::RedbStore, NodeBuilder};
use tokio::task::spawn_blocking;
use uniffi::Record;

use crate::error::{LuminaError, Result};

/// Configuration options for the Lumina node
#[derive(Debug, Clone, Record)]
pub struct NodeConfig {
    /// Base path for storing node data as a string
    pub base_path: String,
    /// Network to connect to
    pub network: network::Network,
    /// Custom list of bootstrap peers to connect to.
    /// If None, uses the canonical bootnodes for the network.
    pub bootnodes: Option<Vec<String>>,
    /// Custom syncing window in seconds. Default is 30 days.
    pub syncing_window_secs: Option<u32>,
    /// Custom pruning delay after syncing window in seconds. Default is 1 hour.
    pub pruning_delay_secs: Option<u32>,
    /// Maximum number of headers in batch while syncing. Default is 128.
    pub batch_size: Option<u64>,
    /// Optional Set the keypair to be used as Node's identity. If None, generates a new Ed25519 keypair.
    pub ed25519_secret_key_bytes: Option<Vec<u8>>,
}

impl NodeConfig {
    /// Convert into NodeBuilder for the implementation
    pub(crate) async fn into_node_builder(self) -> Result<NodeBuilder<RedbBlockstore, RedbStore>> {
        let network_id = self.network.id();
        let base_path = PathBuf::from(self.base_path);
        let store_path = base_path.join(format!("store-{}", network_id));

        spawn_blocking(move || {
            std::fs::create_dir_all(&base_path).map_err(|e| {
                LuminaError::storage(format!("Failed to create base directory: {}", e))
            })
        })
        .await
        .map_err(|e| LuminaError::storage(format!("Failed to create base directory: {}", e)))??;

        let db = spawn_blocking(move || {
            redb::Database::create(&store_path)
                .map(Arc::new)
                .map_err(|e| LuminaError::StorageInit {
                    msg: format!("Failed to create database: {}", e),
                })
        })
        .await
        .map_err(|e| LuminaError::storage(format!("Failed to create base directory: {}", e)))??;

        let store = RedbStore::new(db.clone())
            .await
            .map_err(|e| LuminaError::storage_init(format!("Failed to initialize store: {}", e)))?;

        let blockstore = RedbBlockstore::new(db);

        let bootnodes = if let Some(bootnodes) = self.bootnodes {
            let mut resolved = Vec::with_capacity(bootnodes.len());
            for addr in bootnodes {
                resolved.push(addr.parse()?);
            }
            resolved
        } else {
            self.network.canonical_bootnodes().collect::<Vec<_>>()
        };

        let keypair = if let Some(key_bytes) = self.ed25519_secret_key_bytes {
            if key_bytes.len() != 32 {
                return Err(LuminaError::network("Ed25519 private key must be 32 bytes"));
            }

            Keypair::ed25519_from_bytes(key_bytes)
                .map_err(|e| LuminaError::network(format!("Invalid Ed25519 key: {}", e)))?
        } else {
            libp2p::identity::Keypair::generate_ed25519()
        };

        let mut builder = NodeBuilder::new()
            .store(store)
            .blockstore(blockstore)
            .network(self.network)
            .bootnodes(bootnodes)
            .keypair(keypair)
            .sync_batch_size(self.batch_size.unwrap_or(128));

        if let Some(secs) = self.syncing_window_secs {
            builder = builder.sampling_window(Duration::from_secs(secs.into()));
        }

        if let Some(secs) = self.pruning_delay_secs {
            builder = builder.pruning_delay(Duration::from_secs(secs.into()));
        }

        Ok(builder)
    }
}
