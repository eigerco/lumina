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

    /// A required setting wasn't provided.
    #[error("Required setting not provided: {0}")]
    SettingMissing(String),
}

/// Node conifguration.
pub struct NodeBuilder<B, S>
where
    B: Blockstore,
    S: Store,
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
    blockstore: Option<B>,
    /// The store for headers.
    store: Option<S>,
    /// A handle for the [`sled::Db`] to initialize default sled store and blockstore
    /// within the same instance.
    #[cfg(not(target_arch = "wasm32"))]
    sled_db: Option<sled::Db>,
}

impl<B, S> Default for NodeBuilder<B, S>
where
    B: Blockstore,
    S: Store,
{
    fn default() -> Self {
        Self {
            network: None,
            genesis_hash: None,
            p2p_local_keypair: None,
            p2p_bootnodes: vec![],
            p2p_listen_on: vec![],
            blockstore: None,
            store: None,
            #[cfg(not(target_arch = "wasm32"))]
            sled_db: None,
        }
    }
}

impl<B, S> NodeBuilder<B, S>
where
    B: Blockstore + 'static,
    S: Store,
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
        self.blockstore = Some(blockstore);
        self
    }

    pub fn with_store(mut self, store: S) -> Self {
        self.store = Some(store);
        self
    }

    pub fn from_network(network: Network) -> Self {
        Self::new()
            .with_network(network)
            .with_genesis(network.genesis())
            .with_bootnodes(network.canonical_bootnodes().collect())
    }

    pub async fn build(self) -> Result<Node<S>> {
        let network = self
            .network
            .ok_or_else(|| NodeBuilderError::SettingMissing("network".into()))?;
        let blockstore = self
            .blockstore
            .ok_or_else(|| NodeBuilderError::SettingMissing("blockstore".into()))?;
        let store = self
            .store
            .map(Arc::new)
            .ok_or_else(|| NodeBuilderError::SettingMissing("store".into()))?;

        let local_keypair = self
            .p2p_local_keypair
            .unwrap_or_else(Keypair::generate_ed25519);

        let p2p = Arc::new(P2p::start(P2pArgs {
            network,
            local_keypair,
            bootnodes: self.p2p_bootnodes,
            listen_on: self.p2p_listen_on,
            blockstore,
            store: store.clone(),
        })?);

        let syncer = Arc::new(Syncer::start(SyncerArgs {
            genesis_hash: self.genesis_hash,
            store: store.clone(),
            p2p: p2p.clone(),
        })?);

        Ok(Node::new(p2p, syncer, store))
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

    impl<B, S> NodeBuilder<B, S>
    where
        B: Blockstore,
        S: Store,
    {
        async fn get_sled_db(&mut self) -> Result<sled::Db> {
            if let Some(db) = self.sled_db.clone() {
                return Ok(db);
            }

            let network_id = self
                .network
                .ok_or_else(|| NodeBuilderError::SettingMissing("network".into()))?
                .id();
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

            self.sled_db = Some(db.clone());

            Ok(db)
        }
    }

    impl<S> NodeBuilder<SledBlockstore, S>
    where
        S: Store,
    {
        pub async fn with_default_blockstore(mut self) -> Result<Self> {
            let db = self.get_sled_db().await?;
            self.blockstore = Some(SledBlockstore::new(db).await?);
            Ok(self)
        }
    }

    impl<B> NodeBuilder<B, SledStore>
    where
        B: Blockstore,
    {
        pub async fn with_default_store(mut self) -> Result<Self> {
            let db = self.get_sled_db().await?;
            self.store = Some(SledStore::new(db).await?);
            Ok(self)
        }
    }

    impl NodeBuilder<SledBlockstore, SledStore> {
        pub async fn from_network_with_defaults(network: Network) -> Result<Self> {
            Self::from_network(network)
                .with_default_blockstore()
                .await?
                .with_default_store()
                .await
        }
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
