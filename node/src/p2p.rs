//! Component responsible for the messaging and interacting with other nodes on the peer to peer layer.
//!
//! It is a high level integration of various p2p protocols used by Celestia nodes.
//! Currently supporting:
//! - libp2p-identitfy
//! - libp2p-kad
//! - libp2p-autonat
//! - libp2p-ping
//! - header-sub topic on libp2p-gossipsub
//! - fraud-sub topic on libp2p-gossipsub
//! - header-ex client
//! - header-ex server
//! - bitswap 1.2.0
//! - shwap - celestia's data availability protocol on top of bitswap

use std::collections::HashMap;
use std::future::poll_fn;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use blockstore::Blockstore;
use celestia_proto::p2p::pb::{header_request, HeaderRequest};
use celestia_types::fraud_proof::BadEncodingFraudProof;
use celestia_types::hash::Hash;
use celestia_types::nmt::{Namespace, NamespacedSha2Hasher};
use celestia_types::row::{Row, RowId};
use celestia_types::row_namespace_data::{RowNamespaceData, RowNamespaceDataId};
use celestia_types::sample::{Sample, SampleId};
use celestia_types::{Blob, ExtendedHeader, FraudProof};
use cid::Cid;
use futures::stream::FuturesOrdered;
use futures::TryStreamExt;
use libp2p::{
    gossipsub::{self, TopicHash},
    identity::Keypair,
    swarm::{NetworkBehaviour, NetworkInfo},
    Multiaddr, PeerId,
};
use lumina_utils::executor::{spawn, JoinHandle};
use lumina_utils::time::{self, Interval};
use lumina_utils::token::Token;
use smallvec::SmallVec;
use tendermint_proto::Protobuf;
use tokio::select;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument, trace, warn};

mod connection_control;
mod header_ex;
pub(crate) mod header_session;
pub(crate) mod shwap;
mod swarm;
mod swarm_manager;

use crate::block_ranges::BlockRange;
use crate::events::EventPublisher;
use crate::p2p::header_ex::{HeaderExBehaviour, HeaderExConfig};
use crate::p2p::header_session::HeaderSession;
use crate::p2p::shwap::{convert_cid, get_block_container, ShwapMultihasher};
use crate::p2p::swarm_manager::SwarmManager;
use crate::peer_tracker::PeerTracker;
use crate::peer_tracker::PeerTrackerInfo;
use crate::store::{Store, StoreError};
use crate::utils::{
    celestia_protocol_id, fraudsub_ident_topic, gossipsub_ident_topic, MultiaddrExt,
    OneshotResultSender, OneshotResultSenderExt, OneshotSenderExt,
};

pub use crate::p2p::header_ex::HeaderExError;

// Maximum size of a [`Multihash`].
pub(crate) const MAX_MH_SIZE: usize = 64;

// all fraud proofs for height bigger than head height by this threshold
// will be ignored
const FRAUD_PROOF_HEAD_HEIGHT_THRESHOLD: u64 = 20;

pub(crate) type Result<T, E = P2pError> = std::result::Result<T, E>;

/// Representation of all the errors that can occur in `P2p` component.
#[derive(Debug, thiserror::Error)]
pub enum P2pError {
    /// Failed to initialize gossipsub behaviour.
    #[error("Failed to initialize gossipsub behaviour: {0}")]
    GossipsubInit(String),

    /// Failed to initialize TLS.
    #[error("Failed to initialize TLS: {0}")]
    TlsInit(String),

    /// Failed to initialize noise protocol.
    #[error("Failed to initialize noise: {0}")]
    NoiseInit(String),

    /// The worker has died.
    #[error("Worker died")]
    WorkerDied,

    /// Channel closed unexpectedly.
    #[error("Channel closed unexpectedly")]
    ChannelClosedUnexpectedly,

    /// Not connected to any peers.
    #[error("Not connected to any peers")]
    NoConnectedPeers,

    /// An error propagated from the `header-ex`.
    #[error("HeaderEx: {0}")]
    HeaderEx(#[from] HeaderExError),

    /// Bootnode address is missing its peer ID.
    #[error("Bootnode multiaddrs without peer ID: {0:?}")]
    BootnodeAddrsWithoutPeerId(Vec<Multiaddr>),

    /// An error propagated from [`beetswap::Behaviour`].
    #[error("Bitswap: {0}")]
    Bitswap(#[from] beetswap::Error),

    /// ProtoBuf message failed to be decoded.
    #[error("ProtoBuf decoding error: {0}")]
    ProtoDecodeFailed(#[from] tendermint_proto::Error),

    /// An error propagated from [`celestia_types`] that is related to [`Cid`].
    #[error("CID error: {0}")]
    Cid(celestia_types::Error),

    /// Bitswap query timed out.
    #[error("Bitswap query timed out")]
    BitswapQueryTimeout,

    /// Shwap protocol error.
    #[error("Shwap: {0}")]
    Shwap(String),

    /// An error propagated from [`celestia_types`].
    #[error(transparent)]
    CelestiaTypes(#[from] celestia_types::Error),

    /// An error propagated from [`Store`].
    #[error("Store error: {0}")]
    Store(#[from] StoreError),

    /// Header was pruned.
    #[error("Header of {0} block was pruned because it is outside of retention period")]
    HeaderPruned(u64),

    /// Header not synced yet.
    #[error("Header of {0} block is not synced yet")]
    HeaderNotSynced(u64),
}

impl P2pError {
    /// Returns `true` if an error is fatal in all possible scenarios.
    ///
    /// If unsure mark it as non-fatal error.
    pub(crate) fn is_fatal(&self) -> bool {
        match self {
            P2pError::GossipsubInit(_)
            | P2pError::NoiseInit(_)
            | P2pError::TlsInit(_)
            | P2pError::WorkerDied
            | P2pError::ChannelClosedUnexpectedly
            | P2pError::BootnodeAddrsWithoutPeerId(_) => true,
            P2pError::NoConnectedPeers
            | P2pError::HeaderEx(_)
            | P2pError::Bitswap(_)
            | P2pError::ProtoDecodeFailed(_)
            | P2pError::Cid(_)
            | P2pError::BitswapQueryTimeout
            | P2pError::Shwap(_)
            | P2pError::CelestiaTypes(_)
            | P2pError::HeaderPruned(_)
            | P2pError::HeaderNotSynced(_) => false,
            P2pError::Store(e) => e.is_fatal(),
        }
    }
}

impl From<oneshot::error::RecvError> for P2pError {
    fn from(_value: oneshot::error::RecvError) -> Self {
        P2pError::ChannelClosedUnexpectedly
    }
}

impl From<prost::DecodeError> for P2pError {
    fn from(value: prost::DecodeError) -> Self {
        P2pError::ProtoDecodeFailed(tendermint_proto::Error::decode_message(value))
    }
}

impl From<cid::Error> for P2pError {
    fn from(value: cid::Error) -> Self {
        P2pError::Cid(celestia_types::Error::CidError(
            blockstore::block::CidError::InvalidCid(value.to_string()),
        ))
    }
}

/// Component responsible for the peer to peer networking handling.
#[derive(Debug)]
pub(crate) struct P2p {
    cancellation_token: CancellationToken,
    cmd_tx: mpsc::Sender<P2pCmd>,
    join_handle: JoinHandle,
    peer_tracker_info_watcher: watch::Receiver<PeerTrackerInfo>,
    local_peer_id: PeerId,
}

/// Arguments used to configure the [`P2p`].
pub struct P2pArgs<B, S>
where
    B: Blockstore,
    S: Store,
{
    /// An id of the network to connect to.
    pub network_id: String,
    /// The keypair to be used as the identity.
    pub local_keypair: Keypair,
    /// List of bootstrap nodes to connect to and trust.
    pub bootnodes: Vec<Multiaddr>,
    /// List of the addresses on which to listen for incoming connections.
    pub listen_on: Vec<Multiaddr>,
    /// The store for headers.
    pub blockstore: Arc<B>,
    /// The store for headers.
    pub store: Arc<S>,
    /// Event publisher.
    pub event_pub: EventPublisher,
}

#[derive(Debug)]
pub(crate) enum P2pCmd {
    NetworkInfo {
        respond_to: oneshot::Sender<NetworkInfo>,
    },
    HeaderExRequest {
        request: HeaderRequest,
        respond_to: OneshotResultSender<Vec<ExtendedHeader>, P2pError>,
    },
    Listeners {
        respond_to: oneshot::Sender<Vec<Multiaddr>>,
    },
    ConnectedPeers {
        respond_to: oneshot::Sender<Vec<PeerId>>,
    },
    InitHeaderSub {
        head: Box<ExtendedHeader>,
        /// Any valid headers received by header-sub will be send to this channel.
        channel: mpsc::Sender<ExtendedHeader>,
    },
    SetPeerTrust {
        peer_id: PeerId,
        is_trusted: bool,
    },
    GetShwapCid {
        cid: Cid,
        respond_to: OneshotResultSender<Vec<u8>, P2pError>,
    },
    GetNetworkCompromisedToken {
        respond_to: oneshot::Sender<Token>,
    },
    GetNetworkHead {
        respond_to: oneshot::Sender<Option<ExtendedHeader>>,
    },
}

impl P2p {
    /// Creates and starts a new p2p handler.
    pub async fn start<B, S>(args: P2pArgs<B, S>) -> Result<Self>
    where
        B: Blockstore + 'static,
        S: Store + 'static,
    {
        validate_bootnode_addrs(&args.bootnodes)?;

        let local_peer_id = PeerId::from(args.local_keypair.public());

        let peer_tracker = PeerTracker::new(args.event_pub.clone());
        let peer_tracker_info_watcher = peer_tracker.info_watcher();

        let cancellation_token = CancellationToken::new();
        let (cmd_tx, cmd_rx) = mpsc::channel(16);

        let mut worker =
            Worker::new(args, cancellation_token.child_token(), cmd_rx, peer_tracker).await?;

        let join_handle = spawn(async move {
            worker.run().await;
        });

        Ok(P2p {
            cancellation_token,
            cmd_tx,
            join_handle,
            peer_tracker_info_watcher,
            local_peer_id,
        })
    }

    /// Creates and starts a new mocked p2p handler.
    #[cfg(test)]
    pub fn mocked() -> (Self, crate::test_utils::MockP2pHandle) {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (peer_tracker_tx, peer_tracker_rx) = watch::channel(PeerTrackerInfo::default());
        let cancellation_token = CancellationToken::new();

        // Just a fake join_handle
        let join_handle = spawn(async {});

        let p2p = P2p {
            cmd_tx: cmd_tx.clone(),
            cancellation_token,
            join_handle,
            peer_tracker_info_watcher: peer_tracker_rx,
            local_peer_id: PeerId::random(),
        };

        let handle = crate::test_utils::MockP2pHandle {
            cmd_tx,
            cmd_rx,
            header_sub_tx: None,
            peer_tracker_tx,
        };

        (p2p, handle)
    }

    /// Stop the worker.
    pub fn stop(&self) {
        // Singal the Worker to stop.
        self.cancellation_token.cancel();
    }

    /// Wait until worker is completely stopped.
    pub async fn join(&self) {
        self.join_handle.join().await;
    }

    /// Local peer ID on the p2p network.
    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }

    async fn send_command(&self, cmd: P2pCmd) -> Result<()> {
        self.cmd_tx
            .send(cmd)
            .await
            .map_err(|_| P2pError::WorkerDied)
    }

    /// Watcher for the current [`PeerTrackerInfo`].
    pub fn peer_tracker_info_watcher(&self) -> watch::Receiver<PeerTrackerInfo> {
        self.peer_tracker_info_watcher.clone()
    }

    /// A reference to the current [`PeerTrackerInfo`].
    pub fn peer_tracker_info(&self) -> watch::Ref<'_, PeerTrackerInfo> {
        self.peer_tracker_info_watcher.borrow()
    }

    /// Initializes `header-sub` protocol with a given `subjective_head`.
    pub async fn init_header_sub(
        &self,
        head: ExtendedHeader,
        channel: mpsc::Sender<ExtendedHeader>,
    ) -> Result<()> {
        self.send_command(P2pCmd::InitHeaderSub {
            head: Box::new(head),
            channel,
        })
        .await
    }

    /// Wait until the node is connected to any peer.
    pub async fn wait_connected(&self) -> Result<()> {
        self.peer_tracker_info_watcher()
            .wait_for(|info| info.num_connected_peers > 0)
            .await
            .map(drop)
            .map_err(|_| P2pError::WorkerDied)
    }

    /// Wait until the node is connected to any trusted peer.
    pub async fn wait_connected_trusted(&self) -> Result<()> {
        self.peer_tracker_info_watcher()
            .wait_for(|info| info.num_connected_trusted_peers > 0)
            .await
            .map(drop)
            .map_err(|_| P2pError::WorkerDied)
    }

    /// Get current [`NetworkInfo`].
    pub async fn network_info(&self) -> Result<NetworkInfo> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::NetworkInfo { respond_to: tx })
            .await?;

        Ok(rx.await?)
    }

    /// Send a request on the `header-ex` protocol.
    pub async fn header_ex_request(&self, request: HeaderRequest) -> Result<Vec<ExtendedHeader>> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::HeaderExRequest {
            request,
            respond_to: tx,
        })
        .await?;

        rx.await?
    }

    /// Request the head header on the `header-ex` protocol.
    pub async fn get_head_header(&self) -> Result<ExtendedHeader> {
        self.get_header_by_height(0).await
    }

    /// Request the header by hash on the `header-ex` protocol.
    pub async fn get_header(&self, hash: Hash) -> Result<ExtendedHeader> {
        self.header_ex_request(HeaderRequest {
            data: Some(header_request::Data::Hash(hash.as_bytes().to_vec())),
            amount: 1,
        })
        .await?
        .into_iter()
        .next()
        .ok_or(HeaderExError::HeaderNotFound.into())
    }

    /// Request the header by height on the `header-ex` protocol.
    pub async fn get_header_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        self.header_ex_request(HeaderRequest {
            data: Some(header_request::Data::Origin(height)),
            amount: 1,
        })
        .await?
        .into_iter()
        .next()
        .ok_or(HeaderExError::HeaderNotFound.into())
    }

    /// Request the headers following the one given with the `header-ex` protocol.
    ///
    /// First header from the requested range will be verified against the provided one,
    /// then each subsequent is verified against the previous one.
    pub async fn get_verified_headers_range(
        &self,
        from: &ExtendedHeader,
        amount: u64,
    ) -> Result<Vec<ExtendedHeader>> {
        // User can give us a bad header, so validate it.
        from.validate().map_err(|_| HeaderExError::InvalidRequest)?;

        let height = from.height().value() + 1;

        let range = height..=height + amount - 1;

        let mut session = HeaderSession::new(range, self.cmd_tx.clone());
        let headers = session.run().await?;

        // `.validate()` is called on each header separately by `HeaderExClientHandler`.
        //
        // The last step is to verify that all headers are from the same chain
        // and indeed connected with the next one.
        from.verify_adjacent_range(&headers)
            .map_err(|_| HeaderExError::InvalidResponse)?;

        Ok(headers)
    }

    /// Request a list of ranges with the `header-ex` protocol
    ///
    /// For each of the ranges, headers are verified against each other, but it's the caller
    /// responsibility to verify range edges against headers existing in the store.
    pub(crate) async fn get_unverified_header_range(
        &self,
        range: BlockRange,
    ) -> Result<Vec<ExtendedHeader>> {
        if range.is_empty() {
            return Err(HeaderExError::InvalidRequest.into());
        }

        let mut session = HeaderSession::new(range, self.cmd_tx.clone());
        let headers = session.run().await?;

        let Some(head) = headers.first() else {
            return Err(HeaderExError::InvalidResponse.into());
        };

        head.verify_adjacent_range(&headers[1..])
            .map_err(|_| HeaderExError::InvalidResponse)?;

        Ok(headers)
    }

    /// Request a [`Cid`] on bitswap protocol.
    pub(crate) async fn get_shwap_cid(
        &self,
        cid: Cid,
        timeout: Option<Duration>,
    ) -> Result<Vec<u8>> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::GetShwapCid {
            cid,
            respond_to: tx,
        })
        .await?;

        let data = match timeout {
            Some(dur) => time::timeout(dur, rx)
                .await
                .map_err(|_| P2pError::BitswapQueryTimeout)???,
            None => rx.await??,
        };

        get_block_container(&cid, &data)
    }

    /// Request a [`Row`] on bitswap protocol.
    pub async fn get_row(
        &self,
        row_index: u16,
        block_height: u64,
        timeout: Option<Duration>,
    ) -> Result<Row> {
        let id = RowId::new(row_index, block_height).map_err(P2pError::Cid)?;
        let cid = convert_cid(&id.into())?;

        let data = self.get_shwap_cid(cid, timeout).await?;
        let row = Row::decode(id, &data[..]).map_err(|e| P2pError::Shwap(e.to_string()))?;
        Ok(row)
    }

    /// Request a [`Sample`] on bitswap protocol.
    pub async fn get_sample(
        &self,
        row_index: u16,
        column_index: u16,
        block_height: u64,
        timeout: Option<Duration>,
    ) -> Result<Sample> {
        let id = SampleId::new(row_index, column_index, block_height).map_err(P2pError::Cid)?;
        let cid = convert_cid(&id.into())?;

        let data = self.get_shwap_cid(cid, timeout).await?;
        let sample = Sample::decode(id, &data[..]).map_err(|e| P2pError::Shwap(e.to_string()))?;
        Ok(sample)
    }

    /// Request a [`RowNamespaceData`] on bitswap protocol.
    pub async fn get_row_namespace_data(
        &self,
        namespace: Namespace,
        row_index: u16,
        block_height: u64,
        timeout: Option<Duration>,
    ) -> Result<RowNamespaceData> {
        let id =
            RowNamespaceDataId::new(namespace, row_index, block_height).map_err(P2pError::Cid)?;
        let cid = convert_cid(&id.into())?;

        let data = self.get_shwap_cid(cid, timeout).await?;
        let row_namespace_data =
            RowNamespaceData::decode(id, &data[..]).map_err(|e| P2pError::Shwap(e.to_string()))?;
        Ok(row_namespace_data)
    }

    /// Request all blobs with provided namespace in the block corresponding to this header
    /// using bitswap protocol.
    pub async fn get_all_blobs<S>(
        &self,
        namespace: Namespace,
        block_height: u64,
        timeout: Option<Duration>,
        store: &S,
    ) -> Result<Vec<Blob>>
    where
        S: Store,
    {
        let header = match store.get_by_height(block_height).await {
            Ok(header) => header,
            Err(StoreError::NotFound) => {
                let pruned_ranges = store.get_pruned_ranges().await?;

                if pruned_ranges.contains(block_height) {
                    return Err(P2pError::HeaderPruned(block_height));
                } else {
                    return Err(P2pError::HeaderNotSynced(block_height));
                }
            }
            Err(e) => return Err(e.into()),
        };

        let app_version = header.app_version()?;
        let rows_to_fetch: Vec<_> = header
            .dah
            .row_roots()
            .iter()
            .enumerate()
            .filter(|(_, row)| row.contains::<NamespacedSha2Hasher>(*namespace))
            .map(|(n, _)| n as u16)
            .collect();

        let futs = rows_to_fetch
            .into_iter()
            .map(|row_idx| self.get_row_namespace_data(namespace, row_idx, block_height, timeout))
            .collect::<FuturesOrdered<_>>();

        let rows: Vec<_> = match futs.try_collect().await {
            Ok(rows) => rows,
            Err(P2pError::BitswapQueryTimeout) if !store.has_at(block_height).await => {
                return Err(P2pError::HeaderPruned(block_height));
            }
            Err(e) => return Err(e),
        };

        let shares = rows.iter().flat_map(|row| row.shares.iter());

        Ok(Blob::reconstruct_all(shares, app_version)?)
    }

    /// Get the addresses where [`P2p`] listens on for incoming connections.
    pub async fn listeners(&self) -> Result<Vec<Multiaddr>> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::Listeners { respond_to: tx })
            .await?;

        Ok(rx.await?)
    }

    /// Get the list of connected peers.
    pub async fn connected_peers(&self) -> Result<Vec<PeerId>> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::ConnectedPeers { respond_to: tx })
            .await?;

        Ok(rx.await?)
    }

    /// Alter the trust status for a given peer.
    pub async fn set_peer_trust(&self, peer_id: PeerId, is_trusted: bool) -> Result<()> {
        self.send_command(P2pCmd::SetPeerTrust {
            peer_id,
            is_trusted,
        })
        .await
    }

    /// Get the cancellation token which will be cancelled when the network gets compromised.
    ///
    /// After this token is cancelled, the network should be treated as insincere
    /// and should not be trusted.
    pub(crate) async fn get_network_compromised_token(&self) -> Result<Token> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::GetNetworkCompromisedToken { respond_to: tx })
            .await?;

        Ok(rx.await?)
    }

    /// Get the latest header announced on the network.
    pub async fn get_network_head(&self) -> Result<Option<ExtendedHeader>> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::GetNetworkHead { respond_to: tx })
            .await?;

        Ok(rx.await?)
    }
}

impl Drop for P2p {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(NetworkBehaviour)]
struct Behaviour<B, S>
where
    B: Blockstore + 'static,
    S: Store + 'static,
{
    bitswap: beetswap::Behaviour<MAX_MH_SIZE, B>,
    header_ex: HeaderExBehaviour<S>,
    gossipsub: gossipsub::Behaviour,
}

struct Worker<B, S>
where
    B: Blockstore + 'static,
    S: Store + 'static,
{
    cancellation_token: CancellationToken,
    swarm: SwarmManager<Behaviour<B, S>>,
    header_sub_topic_hash: TopicHash,
    bad_encoding_fraud_sub_topic: TopicHash,
    cmd_rx: mpsc::Receiver<P2pCmd>,
    header_sub_state: Option<HeaderSubState>,
    bitswap_queries: HashMap<beetswap::QueryId, OneshotResultSender<Vec<u8>, P2pError>>,
    network_compromised_token: Token,
    store: Arc<S>,
}

struct HeaderSubState {
    known_head: ExtendedHeader,
    channel: mpsc::Sender<ExtendedHeader>,
}

impl<B, S> Worker<B, S>
where
    B: Blockstore,
    S: Store,
{
    async fn new(
        args: P2pArgs<B, S>,
        cancellation_token: CancellationToken,
        cmd_rx: mpsc::Receiver<P2pCmd>,
        peer_tracker: PeerTracker,
    ) -> Result<Self, P2pError> {
        let header_sub_topic = gossipsub_ident_topic(&args.network_id, "/header-sub/v0.0.1");
        let bad_encoding_fraud_sub_topic =
            fraudsub_ident_topic(BadEncodingFraudProof::TYPE, &args.network_id);
        let gossipsub = init_gossipsub(&args, [&header_sub_topic, &bad_encoding_fraud_sub_topic])?;

        let bitswap = init_bitswap(
            args.blockstore.clone(),
            args.store.clone(),
            &args.network_id,
        )?;

        let header_ex = HeaderExBehaviour::new(HeaderExConfig {
            network_id: &args.network_id,
            header_store: args.store.clone(),
        });

        let behaviour = Behaviour {
            bitswap,
            gossipsub,
            header_ex,
        };

        let swarm = SwarmManager::new(
            &args.network_id,
            &args.local_keypair,
            &args.bootnodes,
            &args.listen_on,
            peer_tracker,
            args.event_pub,
            behaviour,
        )
        .await?;

        Ok(Worker {
            cancellation_token,
            swarm,
            cmd_rx,
            bad_encoding_fraud_sub_topic: bad_encoding_fraud_sub_topic.hash(),
            header_sub_topic_hash: header_sub_topic.hash(),
            header_sub_state: None,
            bitswap_queries: HashMap::new(),
            network_compromised_token: Token::new(),
            store: args.store,
        })
    }

    async fn run(&mut self) {
        let mut report_interval = Interval::new(Duration::from_secs(60)).await;

        loop {
            select! {
                _ = self.cancellation_token.cancelled() => break,
                _ = report_interval.tick() => {
                    self.report();
                }
                _ = poll_closed(&mut self.bitswap_queries) => {
                    self.prune_canceled_bitswap_queries();
                }
                res = self.swarm.poll() => {
                    match res {
                        Ok(ev) => {
                            if let Err(e) = self.on_behaviour_event(ev).await {
                                warn!("Failure while handling behaviour event: {e}");
                            }
                        }
                        Err(e) => warn!("Failure while polling SwarmManager: {e}"),
                    }
                }
                Some(cmd) = self.cmd_rx.recv() => {
                    if let Err(e) = self.on_cmd(cmd).await {
                        warn!("Failure while handling command. (error: {e})");
                    }
                }
            }
        }

        self.swarm.context().behaviour.header_ex.stop();
        self.swarm.stop().await;
    }

    fn prune_canceled_bitswap_queries(&mut self) {
        let mut cancelled = SmallVec::<[_; 16]>::new();

        for (query_id, chan) in &self.bitswap_queries {
            if chan.is_closed() {
                cancelled.push(*query_id);
            }
        }

        for query_id in cancelled {
            self.bitswap_queries.remove(&query_id);
            self.swarm.context().behaviour.bitswap.cancel(query_id);
        }
    }

    async fn on_behaviour_event(&mut self, ev: BehaviourEvent<B, S>) -> Result<()> {
        match ev {
            BehaviourEvent::Gossipsub(ev) => self.on_gossip_sub_event(ev).await,
            BehaviourEvent::Bitswap(ev) => self.on_bitswap_event(ev).await,
            BehaviourEvent::HeaderEx(_) => {}
        }

        Ok(())
    }

    async fn on_cmd(&mut self, cmd: P2pCmd) -> Result<()> {
        match cmd {
            P2pCmd::NetworkInfo { respond_to } => {
                respond_to.maybe_send(self.swarm.network_info());
            }
            P2pCmd::HeaderExRequest {
                request,
                respond_to,
            } => {
                let ctx = self.swarm.context();
                ctx.behaviour
                    .header_ex
                    .send_request(request, respond_to, ctx.peer_tracker);
            }
            P2pCmd::Listeners { respond_to } => {
                respond_to.maybe_send(self.swarm.listeners());
            }
            P2pCmd::ConnectedPeers { respond_to } => {
                let peers = self
                    .swarm
                    .context()
                    .peer_tracker
                    .peers()
                    .filter_map(|peer| {
                        if peer.is_connected() {
                            Some(*peer.id())
                        } else {
                            None
                        }
                    })
                    .collect();
                respond_to.maybe_send(peers);
            }
            P2pCmd::InitHeaderSub { head, channel } => {
                self.on_init_header_sub(*head, channel);
            }
            P2pCmd::SetPeerTrust {
                peer_id,
                is_trusted,
            } => {
                self.swarm.set_peer_trust(&peer_id, is_trusted);
            }
            P2pCmd::GetShwapCid { cid, respond_to } => {
                self.on_get_shwap_cid(cid, respond_to);
            }
            P2pCmd::GetNetworkCompromisedToken { respond_to } => {
                respond_to.maybe_send(self.network_compromised_token.clone())
            }
            P2pCmd::GetNetworkHead { respond_to } => {
                let head = self
                    .header_sub_state
                    .as_ref()
                    .map(|state| state.known_head.clone());
                respond_to.maybe_send(head);
            }
        }

        Ok(())
    }

    #[instrument(skip_all)]
    fn report(&mut self) {
        let tracker_info = self.swarm.context().peer_tracker.info();

        info!(
            "peers: {}, trusted peers: {}",
            tracker_info.num_connected_peers, tracker_info.num_connected_trusted_peers,
        );
    }

    #[instrument(level = "trace", skip(self))]
    async fn on_gossip_sub_event(&mut self, ev: gossipsub::Event) {
        match ev {
            gossipsub::Event::Message {
                message,
                message_id,
                ..
            } => {
                let Some(peer) = message.source else {
                    // Validation mode is `strict` so this will never happen
                    return;
                };

                let acceptance = if message.topic == self.header_sub_topic_hash {
                    self.on_header_sub_message(&message.data[..])
                } else if message.topic == self.bad_encoding_fraud_sub_topic {
                    self.on_bad_encoding_fraud_sub_message(&message.data[..], &peer)
                        .await
                } else {
                    trace!("Unhandled gossipsub message");
                    gossipsub::MessageAcceptance::Ignore
                };

                if !matches!(acceptance, gossipsub::MessageAcceptance::Reject) {
                    // We may have discovered a new peer
                    self.swarm.peer_maybe_discovered(&peer);
                }

                let _ = self
                    .swarm
                    .context()
                    .behaviour
                    .gossipsub
                    .report_message_validation_result(&message_id, &peer, acceptance);
            }
            _ => trace!("Unhandled gossipsub event"),
        }
    }

    #[instrument(level = "trace", skip_all)]
    fn on_get_shwap_cid(&mut self, cid: Cid, respond_to: OneshotResultSender<Vec<u8>, P2pError>) {
        trace!("Requesting CID {cid} from bitswap");
        let query_id = self.swarm.context().behaviour.bitswap.get(&cid);
        self.bitswap_queries.insert(query_id, respond_to);
    }

    #[instrument(level = "trace", skip(self))]
    async fn on_bitswap_event(&mut self, ev: beetswap::Event) {
        match ev {
            beetswap::Event::GetQueryResponse { query_id, data } => {
                if let Some(respond_to) = self.bitswap_queries.remove(&query_id) {
                    respond_to.maybe_send_ok(data);
                }
            }
            beetswap::Event::GetQueryError { query_id, error } => {
                if let Some(respond_to) = self.bitswap_queries.remove(&query_id) {
                    let error: P2pError = error.into();
                    respond_to.maybe_send_err(error);
                }
            }
        }
    }

    #[instrument(skip_all, fields(header = %head))]
    fn on_init_header_sub(&mut self, head: ExtendedHeader, channel: mpsc::Sender<ExtendedHeader>) {
        self.header_sub_state = Some(HeaderSubState {
            known_head: head,
            channel,
        });
        trace!("HeaderSub initialized");
    }

    #[instrument(skip_all)]
    fn on_header_sub_message(&mut self, data: &[u8]) -> gossipsub::MessageAcceptance {
        let Ok(header) = ExtendedHeader::decode_and_validate(data) else {
            trace!("Malformed or invalid header from header-sub");
            return gossipsub::MessageAcceptance::Reject;
        };

        trace!("Received header from header-sub ({header})");

        let Some(ref mut state) = self.header_sub_state else {
            debug!("header-sub not initialized yet");
            return gossipsub::MessageAcceptance::Ignore;
        };

        if state.known_head.verify(&header).is_err() {
            trace!("Failed to verify HeaderSub header. Ignoring {header}");
            return gossipsub::MessageAcceptance::Ignore;
        }

        trace!("New header from header-sub ({header})");

        state.known_head = header.clone();
        // We intentionally do not `send().await` to avoid blocking `P2p`
        // in case `Syncer` enters some weird state.
        let _ = state.channel.try_send(header);

        gossipsub::MessageAcceptance::Accept
    }

    #[instrument(skip_all)]
    async fn on_bad_encoding_fraud_sub_message(
        &mut self,
        data: &[u8],
        peer: &PeerId,
    ) -> gossipsub::MessageAcceptance {
        let Ok(befp) = BadEncodingFraudProof::decode(data) else {
            trace!("Malformed bad encoding fraud proof from {peer}");
            // TODO
            self.swarm
                .context()
                .behaviour
                .gossipsub
                .blacklist_peer(peer);
            return gossipsub::MessageAcceptance::Reject;
        };

        let height = befp.height().value();

        let current_height = if let Some(ref header_sub_state) = self.header_sub_state {
            header_sub_state.known_head.height().value()
        } else if let Ok(local_head) = self.store.get_head().await {
            local_head.height().value()
        } else {
            // we aren't tracking the network and have uninitialized store
            return gossipsub::MessageAcceptance::Ignore;
        };

        if height > current_height + FRAUD_PROOF_HEAD_HEIGHT_THRESHOLD {
            // does this threshold make any sense if we're gonna ignore it anyway
            // since we won't have the header
            return gossipsub::MessageAcceptance::Ignore;
        }

        let hash = befp.header_hash();
        let Ok(header) = self.store.get_by_hash(&hash).await else {
            // we can't verify the proof without a header
            // TODO: should we then store it and wait for the height? celestia doesn't
            return gossipsub::MessageAcceptance::Ignore;
        };

        if let Err(e) = befp.validate(&header) {
            trace!("Received invalid bad encoding fraud proof from {peer}: {e}");
            self.swarm
                .context()
                .behaviour
                .gossipsub
                .blacklist_peer(peer);
            return gossipsub::MessageAcceptance::Reject;
        }

        warn!("Received a valid bad encoding fraud proof");
        // trigger cancellation for all services
        self.network_compromised_token.trigger();

        gossipsub::MessageAcceptance::Accept
    }
}

/// Awaits at least one channel from the `bitswap_queries` to close.
async fn poll_closed(
    bitswap_queries: &mut HashMap<beetswap::QueryId, OneshotResultSender<Vec<u8>, P2pError>>,
) {
    poll_fn(|cx| {
        for chan in bitswap_queries.values_mut() {
            match chan.poll_closed(cx) {
                Poll::Pending => continue,
                Poll::Ready(_) => return Poll::Ready(()),
            }
        }

        Poll::Pending
    })
    .await
}

fn validate_bootnode_addrs(addrs: &[Multiaddr]) -> Result<(), P2pError> {
    let mut invalid_addrs = Vec::new();

    for addr in addrs {
        if addr.peer_id().is_none() {
            invalid_addrs.push(addr.to_owned());
        }
    }

    if invalid_addrs.is_empty() {
        Ok(())
    } else {
        Err(P2pError::BootnodeAddrsWithoutPeerId(invalid_addrs))
    }
}

fn init_gossipsub<'a, B, S>(
    args: &'a P2pArgs<B, S>,
    topics: impl IntoIterator<Item = &'a gossipsub::IdentTopic>,
) -> Result<gossipsub::Behaviour>
where
    B: Blockstore,
    S: Store,
{
    // Set the message authenticity - How we expect to publish messages
    // Here we expect the publisher to sign the message with their key.
    let message_authenticity = gossipsub::MessageAuthenticity::Signed(args.local_keypair.clone());

    let config = gossipsub::ConfigBuilder::default()
        .validation_mode(gossipsub::ValidationMode::Strict)
        .validate_messages()
        .build()
        .map_err(|e| P2pError::GossipsubInit(e.to_string()))?;

    // build a gossipsub network behaviour
    let mut gossipsub: gossipsub::Behaviour =
        gossipsub::Behaviour::new(message_authenticity, config)
            .map_err(|e| P2pError::GossipsubInit(e.to_string()))?;

    for topic in topics {
        gossipsub
            .subscribe(topic)
            .map_err(|e| P2pError::GossipsubInit(e.to_string()))?;
    }

    Ok(gossipsub)
}

fn init_bitswap<B, S>(
    blockstore: Arc<B>,
    store: Arc<S>,
    network_id: &str,
) -> Result<beetswap::Behaviour<MAX_MH_SIZE, B>>
where
    B: Blockstore + 'static,
    S: Store + 'static,
{
    let protocol_prefix = celestia_protocol_id(network_id, "shwap");

    Ok(beetswap::Behaviour::builder(blockstore)
        .protocol_prefix(protocol_prefix.as_ref())?
        .register_multihasher(ShwapMultihasher::new(store))
        .client_set_send_dont_have(false)
        .build())
}
