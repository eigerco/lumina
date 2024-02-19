use std::io;
use std::sync::Arc;

use blockstore::{Blockstore, BlockstoreError};
use celestia_types::hash::Hash;
use libp2p::identity::Keypair;
use libp2p::Multiaddr;

use crate::network::Network;
use crate::node::Node;
use crate::p2p::{P2p, P2pArgs, P2pError};
use crate::store::{Store, StoreError};
use crate::syncer::{Syncer, SyncerArgs, SyncerError};

type Result<T, E = NodeBuilderError> = std::result::Result<T, E>;

/// Representation of all the errors that can occur when interacting with the [`NodeBuilder`].
#[derive(Debug, thiserror::Error)]
pub enum NodeBuilderError {
    /// An error propagated from the [`P2p`] module.
    #[error(transparent)]
    P2p(#[from] P2pError),

    /// An error propagated from the [`Syncer`] module.
    #[error(transparent)]
    Syncer(#[from] SyncerError),

    /// An error propagated from the [`Blockstore`] module.
    #[error(transparent)]
    BlockstoreError(#[from] BlockstoreError),

    /// An error propagated from the [`Store`] module.
    #[error(transparent)]
    StoreError(#[from] StoreError),

    /// An error propagated from the IO operation.
    #[error("Received io error from persistent storage: {0}")]
    IoError(#[from] io::Error),

    /// Network was required but not provided.
    #[error("Network not provided. Consider calling `.with_network`")]
    NetworkMissing,
}

/// Node conifguration.
pub struct NodeBuilder<B>
where
    B: Blockstore + 'static,
{
    /// An id of the network to connect to.
    network: Option<Network>,
    /// The hash of the genesis block in network.
    genesis_hash: Option<Hash>,
    /// The keypair to be used as [`Node`]s identity.
    p2p_local_keypair: Option<Keypair>,
    /// List of bootstrap nodes to connect to and trust.
    p2p_bootnodes: Vec<Multiaddr>,
    /// List of the addresses where [`Node`] will listen for incoming connections.
    p2p_listen_on: Vec<Multiaddr>,
    /// The blockstore for bitswap.
    blockstore: BlockstoreChoice<B>,
    /// The store for headers.
    store: Option<Arc<dyn Store>>,
}

enum BlockstoreChoice<B>
where
    B: Blockstore + 'static,
{
    Default,
    Custom(B),
}

impl<B> BlockstoreChoice<B>
where
    B: Blockstore,
{
    fn is_default(&self) -> bool {
        matches!(self, BlockstoreChoice::Default)
    }
}

impl<B> Default for NodeBuilder<B>
where
    B: Blockstore,
{
    fn default() -> Self {
        Self {
            network: None,
            genesis_hash: None,
            p2p_local_keypair: None,
            p2p_bootnodes: vec![],
            p2p_listen_on: vec![],
            blockstore: BlockstoreChoice::Default,
            store: None,
        }
    }
}

impl<B> NodeBuilder<B>
where
    B: Blockstore + 'static,
{
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_network(mut self, network: Network) -> Self {
        self.network = Some(network);
        self
    }

    pub fn with_genesis(mut self, hash: Option<Hash>) -> Self {
        self.genesis_hash = hash;
        self
    }

    pub fn with_p2p_keypair(mut self, keypair: Keypair) -> Self {
        self.p2p_local_keypair = Some(keypair);
        self
    }

    pub fn with_listeners(mut self, listeners: Vec<Multiaddr>) -> Self {
        self.p2p_listen_on = listeners;
        self
    }

    pub fn with_bootnodes(mut self, bootnodes: Vec<Multiaddr>) -> Self {
        self.p2p_bootnodes = bootnodes;
        self
    }

    pub fn with_blockstore(mut self, blockstore: B) -> Self {
        self.blockstore = BlockstoreChoice::Custom(blockstore);
        self
    }

    pub fn with_store<S: Store + 'static>(mut self, store: S) -> Self {
        self.store = Some(Arc::new(store));
        self
    }

    pub fn from_network(network: Network) -> Self {
        Self::new()
            .with_network(network)
            .with_genesis(network.genesis())
            .with_bootnodes(network.canonical_bootnodes().collect())
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::Path;

    use directories::ProjectDirs;
    use tokio::{fs, task::spawn_blocking};
    use tracing::warn;

    use crate::{blockstore::SledBlockstore, store::SledStore, utils};

    use super::*;

    impl NodeBuilder<SledBlockstore> {
        pub fn with_default_blockstore(mut self) -> Self {
            self.blockstore = BlockstoreChoice::Default;
            self
        }
    }

    impl<B> NodeBuilder<B>
    where
        B: Blockstore + 'static,
    {
        pub async fn build(self) -> Result<Node> {
            let network = self.network.ok_or(NodeBuilderError::NetworkMissing)?;
            let local_keypair = self
                .p2p_local_keypair
                .unwrap_or_else(Keypair::generate_ed25519);

            let default_db = if self.blockstore.is_default() || self.store.is_none() {
                Some(default_sled_db(network).await?)
            } else {
                None
            };
            let store = if let Some(store) = self.store {
                store
            } else {
                Arc::new(SledStore::new(default_db.clone().unwrap()).await?)
            };

            let p2p = if let BlockstoreChoice::Custom(blockstore) = self.blockstore {
                Arc::new(P2p::start(P2pArgs {
                    network,
                    local_keypair,
                    bootnodes: self.p2p_bootnodes,
                    listen_on: self.p2p_listen_on,
                    blockstore,
                    store: store.clone(),
                })?)
            } else {
                Arc::new(P2p::start(P2pArgs {
                    network,
                    local_keypair,
                    bootnodes: self.p2p_bootnodes,
                    listen_on: self.p2p_listen_on,
                    blockstore: SledBlockstore::new(default_db.unwrap()).await?,
                    store: store.clone(),
                })?)
            };

            let syncer = Arc::new(Syncer::start(SyncerArgs {
                genesis_hash: self.genesis_hash,
                store: store.clone(),
                p2p: p2p.clone(),
            })?);

            Ok(Node::new(p2p, syncer, store))
        }
    }

    async fn default_sled_db(network: Network) -> Result<sled::Db> {
        let network_id = network.id();
        let mut data_dir = utils::data_dir()
            .ok_or_else(|| StoreError::OpenFailed("Can't find home of current user".into()))?;

        // TODO(02.2024): remove in 3 months or after few releases
        migrate_from_old_cache_dir(&data_dir).await?;

        data_dir.push(network_id);
        data_dir.push("db");

        let db = spawn_blocking(|| sled::open(data_dir))
            .await
            .map_err(io::Error::from)?
            .map_err(|e| StoreError::OpenFailed(e.to_string()))?;

        Ok(db)
    }

    // TODO(02.2024): remove in 3 months or after few releases
    // Previously we used `.cache/celestia/{network_id}` (or os equivalent) for
    // sled's persistent storage. This function will migrate it to a new lumina
    // data dir. If a new storage is found too, migration will not overwrite it.
    async fn migrate_from_old_cache_dir(data_dir: &Path) -> Result<()> {
        let old_cache_dir = ProjectDirs::from("co", "eiger", "celestia")
            .expect("Must succeed after data_dir is known")
            .cache_dir()
            .to_owned();

        // already migrated or fresh usage
        if !old_cache_dir.exists() {
            return Ok(());
        }

        // we won't migrate old data if user already have a new persistent storage.
        if data_dir.exists() {
            warn!(
                "Found both old and new Lumina storages. {} can be deleted.",
                old_cache_dir.display()
            );
            return Ok(());
        }

        warn!(
            "Migrating Lumina storage to a new location: {} -> {}",
            old_cache_dir.display(),
            data_dir.display()
        );

        // migrate data for each network
        for network in [
            Network::Arabica,
            Network::Mocha,
            Network::Mainnet,
            Network::Private,
        ] {
            let net_id = network.id();
            let old = old_cache_dir.join(net_id);
            let new = data_dir.join(net_id);

            if old.exists() {
                fs::create_dir_all(&new).await?;
                fs::rename(old, new.join("db")).await?;
            }
        }

        if old_cache_dir.read_dir()?.count() > 0 {
            warn!("Old Lumina storage not empty after successful migration.");
            warn!(
                "Inspect and remove it manually: {}",
                old_cache_dir.display()
            );
        }

        Ok(())
    }
}
