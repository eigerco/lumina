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
use std::io;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use blockstore::Blockstore;
use celestia_proto::p2p::pb::{header_request, HeaderRequest};
use celestia_tendermint_proto::Protobuf;
use celestia_types::namespaced_data::NamespacedData;
use celestia_types::nmt::Namespace;
use celestia_types::row::Row;
use celestia_types::sample::Sample;
use celestia_types::{fraud_proof::BadEncodingFraudProof, hash::Hash};
use celestia_types::{ExtendedHeader, FraudProof, Height};
use cid::Cid;
use futures::StreamExt;
use libp2p::{
    autonat,
    core::{ConnectedPoint, Endpoint},
    gossipsub::{self, SubscriptionError, TopicHash},
    identify,
    identity::Keypair,
    kad,
    multiaddr::Protocol,
    ping,
    swarm::{ConnectionId, DialError, NetworkBehaviour, NetworkInfo, Swarm, SwarmEvent},
    Multiaddr, PeerId, TransportError,
};
use smallvec::SmallVec;
use tokio::select;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, instrument, trace, warn};
use web_time::Instant;

mod header_ex;
mod header_session;
pub(crate) mod shwap;
mod swarm;

use crate::executor::{self, spawn, Interval};
use crate::p2p::header_ex::{HeaderExBehaviour, HeaderExConfig};
use crate::p2p::header_session::HeaderSession;
use crate::p2p::shwap::{namespaced_data_cid, row_cid, sample_cid, ShwapMultihasher};
use crate::p2p::swarm::new_swarm;
use crate::peer_tracker::PeerTracker;
use crate::peer_tracker::PeerTrackerInfo;
use crate::store::header_ranges::HeaderRanges;
use crate::store::Store;
use crate::utils::{
    celestia_protocol_id, fraudsub_ident_topic, gossipsub_ident_topic, MultiaddrExt,
    OneshotResultSender, OneshotResultSenderExt, OneshotSenderExt,
};

pub use crate::p2p::header_ex::HeaderExError;

// Minimal number of peers that we want to maintain connection to.
// If we have fewer peers than that, we will try to reconnect / discover
// more aggresively.
const MIN_CONNECTED_PEERS: u64 = 4;

// Bootstrap procedure is a bit misleading as a name. It is actually
// scanning the network thought the already known peers and find new
// ones. It also recovers connectivity of previously known peers and
// refreshes the routing table.
//
// libp2p team suggests to start bootstrap procedure every 5 minute.
const KADEMLIA_BOOTSTRAP_PERIOD: Duration = Duration::from_secs(5 * 60);

// Maximum size of a [`Multihash`].
pub(crate) const MAX_MH_SIZE: usize = 64;

pub(crate) const GET_SAMPLE_TIMEOUT: Duration = Duration::from_secs(10);

// all fraud proofs for height bigger than head height by this threshold
// will be ignored
const FRAUD_PROOF_HEAD_HEIGHT_THRESHOLD: u64 = 20;

type Result<T, E = P2pError> = std::result::Result<T, E>;

/// Representation of all the errors that can occur when interacting with [`P2p`].
#[derive(Debug, thiserror::Error)]
pub enum P2pError {
    /// Failed to initialize gossipsub behaviour.
    #[error("Failed to initialize gossipsub behaviour: {0}")]
    GossipsubInit(String),

    /// Failed to subscribe to a topic on gossibsub.
    #[error("Failed to on gossipsub subscribe: {0}")]
    GossipsubSubscribe(#[from] SubscriptionError),

    /// An error propagated from the libp2p transport.
    #[error("Transport error: {0}")]
    Transport(#[from] TransportError<io::Error>),

    /// Failed to initialize noise protocol.
    #[error("Failed to initialize noise: {0}")]
    InitNoise(String),

    /// Error occured when trying to establish or upgrade an outbound connection.
    #[error("Dial error: {0}")]
    Dial(#[from] DialError),

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
    ProtoDecodeFailed(#[from] celestia_tendermint_proto::Error),

    /// An error propagated from [`celestia_types`] that is related to [`Cid`].
    #[error("CID error: {0}")]
    Cid(celestia_types::Error),

    /// Bitswap query timed out.
    #[error("Bitswap query timed out")]
    BitswapQueryTimeout,
}

impl From<oneshot::error::RecvError> for P2pError {
    fn from(_value: oneshot::error::RecvError) -> Self {
        P2pError::ChannelClosedUnexpectedly
    }
}

/// Component responsible for the peer to peer networking handling.
#[derive(Debug)]
pub struct P2p {
    cmd_tx: mpsc::Sender<P2pCmd>,
    header_sub_watcher: watch::Receiver<Option<ExtendedHeader>>,
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
    pub blockstore: B,
    /// The store for headers.
    pub store: Arc<S>,
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
        respond_to: oneshot::Sender<CancellationToken>,
    },
}

impl P2p {
    /// Creates and starts a new p2p handler.
    pub fn start<B, S>(args: P2pArgs<B, S>) -> Result<Self>
    where
        B: Blockstore + 'static,
        S: Store + 'static,
    {
        validate_bootnode_addrs(&args.bootnodes)?;

        let local_peer_id = PeerId::from(args.local_keypair.public());

        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (header_sub_tx, header_sub_rx) = watch::channel(None);

        let peer_tracker = Arc::new(PeerTracker::new());
        let peer_tracker_info_watcher = peer_tracker.info_watcher();

        let mut worker = Worker::new(args, cmd_rx, header_sub_tx, peer_tracker)?;

        spawn(async move {
            worker.run().await;
        });

        Ok(P2p {
            cmd_tx,
            header_sub_watcher: header_sub_rx,
            peer_tracker_info_watcher,
            local_peer_id,
        })
    }

    /// Creates and starts a new mocked p2p handler.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn mocked() -> (Self, crate::test_utils::MockP2pHandle) {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (header_sub_tx, header_sub_rx) = watch::channel(None);
        let (peer_tracker_tx, peer_tracker_rx) = watch::channel(PeerTrackerInfo::default());

        let p2p = P2p {
            cmd_tx: cmd_tx.clone(),
            header_sub_watcher: header_sub_rx,
            peer_tracker_info_watcher: peer_tracker_rx,
            local_peer_id: PeerId::random(),
        };

        let handle = crate::test_utils::MockP2pHandle {
            cmd_tx,
            cmd_rx,
            header_sub_tx,
            peer_tracker_tx,
        };

        (p2p, handle)
    }

    /// Stop the [`P2p`].
    pub async fn stop(&self) -> Result<()> {
        // TODO
        Ok(())
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

    /// Watcher for the latest verified network head headers announced on `header-sub`.
    pub fn header_sub_watcher(&self) -> watch::Receiver<Option<ExtendedHeader>> {
        self.header_sub_watcher.clone()
    }

    /// Watcher for the current [`PeerTrackerInfo`].
    pub fn peer_tracker_info_watcher(&self) -> watch::Receiver<PeerTrackerInfo> {
        self.peer_tracker_info_watcher.clone()
    }

    /// A reference to the current [`PeerTrackerInfo`].
    pub fn peer_tracker_info(&self) -> watch::Ref<PeerTrackerInfo> {
        self.peer_tracker_info_watcher.borrow()
    }

    /// Initializes `header-sub` protocol with a given `subjective_head`.
    pub async fn init_header_sub(&self, head: ExtendedHeader) -> Result<()> {
        self.send_command(P2pCmd::InitHeaderSub {
            head: Box::new(head),
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
    /// First header from the requested range will be verified against the provided one, then each subsequent is verified against the previous one.
    pub async fn get_verified_headers_range(
        &self,
        from: &ExtendedHeader,
        amount: u64,
    ) -> Result<Vec<ExtendedHeader>> {
        from.validate().map_err(|_| HeaderExError::InvalidRequest)?;

        let height = from.height().value() + 1;

        let range = height..=height + amount - 1;

        let mut session = HeaderSession::new([range].into(), self.cmd_tx.clone())?;
        let headers = session.run().await?.pop().unwrap(); // XXX

        from.verify_adjacent_range(&headers)
            .map_err(|_| HeaderExError::InvalidResponse)?;

        Ok(headers)
    }

    /// Request a list of ranges with the `header-ex` protocol
    ///
    /// For each of the ranges, headers are verified against each other, but it's the caller
    /// responsibility to verify range edges against headers existing in the store.
    pub(crate) async fn get_unverified_header_ranges(
        &self,
        ranges: HeaderRanges,
    ) -> Result<Vec<Vec<ExtendedHeader>>> {
        let mut session = HeaderSession::new(ranges, self.cmd_tx.clone())?;
        let header_ranges = session.run().await?;

        for range in &header_ranges {
            let Some(head) = range.first() else {
                continue;
            };
            head.verify_adjacent_range(&range[1..])
                .map_err(|_| HeaderExError::InvalidResponse)?;
        }

        Ok(header_ranges)
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
            Some(dur) => executor::timeout(dur, rx)
                .await
                .map_err(|_| P2pError::BitswapQueryTimeout)???,
            None => rx.await??,
        };

        Ok(data)
    }

    /// Request a [`Row`] on bitswap protocol.
    pub async fn get_row(&self, row_index: u16, block_height: u64) -> Result<Row> {
        let cid = row_cid(row_index, block_height)?;
        // TODO: add timeout
        let data = self.get_shwap_cid(cid, None).await?;
        Ok(Row::decode(&data[..])?)
    }

    /// Request a [`Sample`] on bitswap protocol.
    ///
    /// This method awaits for a verified `Sample` until timeout of 10 second
    /// is reached. On timeout it is safe to assume that sampling of the block
    /// failed.
    pub async fn get_sample(
        &self,
        row_index: u16,
        column_index: u16,
        block_height: u64,
    ) -> Result<Sample> {
        let cid = sample_cid(row_index, column_index, block_height)?;
        let data = self.get_shwap_cid(cid, Some(GET_SAMPLE_TIMEOUT)).await?;
        Ok(Sample::decode(&data[..])?)
    }

    /// Request a [`NamespacedData`] on bitswap protocol.
    pub async fn get_namespaced_data(
        &self,
        namespace: Namespace,
        row_index: u16,
        block_height: u64,
    ) -> Result<NamespacedData> {
        let cid = namespaced_data_cid(namespace, row_index, block_height)?;
        // TODO: add timeout
        let data = self.get_shwap_cid(cid, None).await?;
        Ok(NamespacedData::decode(&data[..])?)
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
    pub(crate) async fn get_network_compromised_token(&self) -> Result<CancellationToken> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::GetNetworkCompromisedToken { respond_to: tx })
            .await?;

        Ok(rx.await?)
    }
}

/// Our network behaviour.
#[derive(NetworkBehaviour)]
struct Behaviour<B, S>
where
    B: Blockstore + 'static,
    S: Store + 'static,
{
    autonat: autonat::Behaviour,
    bitswap: beetswap::Behaviour<MAX_MH_SIZE, B>,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    header_ex: HeaderExBehaviour<S>,
    gossipsub: gossipsub::Behaviour,
    kademlia: kad::Behaviour<kad::store::MemoryStore>,
}

struct Worker<B, S>
where
    B: Blockstore + 'static,
    S: Store + 'static,
{
    swarm: Swarm<Behaviour<B, S>>,
    header_sub_topic_hash: TopicHash,
    bad_encoding_fraud_sub_topic: TopicHash,
    cmd_rx: mpsc::Receiver<P2pCmd>,
    peer_tracker: Arc<PeerTracker>,
    header_sub_watcher: watch::Sender<Option<ExtendedHeader>>,
    bitswap_queries: HashMap<beetswap::QueryId, OneshotResultSender<Vec<u8>, P2pError>>,
    network_compromised_token: CancellationToken,
    store: Arc<S>,
}

impl<B, S> Worker<B, S>
where
    B: Blockstore,
    S: Store,
{
    fn new(
        args: P2pArgs<B, S>,
        cmd_rx: mpsc::Receiver<P2pCmd>,
        header_sub_watcher: watch::Sender<Option<ExtendedHeader>>,
        peer_tracker: Arc<PeerTracker>,
    ) -> Result<Self, P2pError> {
        let local_peer_id = PeerId::from(args.local_keypair.public());

        let autonat = autonat::Behaviour::new(local_peer_id, autonat::Config::default());
        let ping = ping::Behaviour::new(ping::Config::default());

        let identify = identify::Behaviour::new(identify::Config::new(
            String::new(),
            args.local_keypair.public(),
        ));

        let header_sub_topic = gossipsub_ident_topic(&args.network_id, "/header-sub/v0.0.1");
        let bad_encoding_fraud_sub_topic =
            fraudsub_ident_topic(BadEncodingFraudProof::TYPE, &args.network_id);
        let gossipsub = init_gossipsub(&args, [&header_sub_topic, &bad_encoding_fraud_sub_topic])?;

        let kademlia = init_kademlia(&args)?;
        let bitswap = init_bitswap(args.blockstore, args.store.clone(), &args.network_id)?;

        let header_ex = HeaderExBehaviour::new(HeaderExConfig {
            network_id: &args.network_id,
            peer_tracker: peer_tracker.clone(),
            header_store: args.store.clone(),
        });

        let behaviour = Behaviour {
            autonat,
            bitswap,
            ping,
            identify,
            gossipsub,
            header_ex,
            kademlia,
        };

        let mut swarm = new_swarm(args.local_keypair, behaviour)?;

        for addr in args.listen_on {
            swarm.listen_on(addr)?;
        }

        for addr in args.bootnodes {
            // Bootstrap peers are always trusted
            if let Some(peer_id) = addr.peer_id() {
                peer_tracker.set_trusted(peer_id, true);
            }
            swarm.dial(addr)?;
        }

        Ok(Worker {
            cmd_rx,
            swarm,
            bad_encoding_fraud_sub_topic: bad_encoding_fraud_sub_topic.hash(),
            header_sub_topic_hash: header_sub_topic.hash(),
            peer_tracker,
            header_sub_watcher,
            bitswap_queries: HashMap::new(),
            network_compromised_token: CancellationToken::new(),
            store: args.store,
        })
    }

    async fn run(&mut self) {
        let mut report_interval = Interval::new(Duration::from_secs(60)).await;
        let mut kademlia_interval = Interval::new(Duration::from_secs(30)).await;
        let mut kademlia_last_bootstrap = Instant::now();

        // Initiate discovery
        let _ = self.swarm.behaviour_mut().kademlia.bootstrap();

        loop {
            select! {
                _ = report_interval.tick() => {
                    self.report();
                }
                _ = kademlia_interval.tick() => {
                    if self.peer_tracker.info().num_connected_peers < MIN_CONNECTED_PEERS
                        || kademlia_last_bootstrap.elapsed() > KADEMLIA_BOOTSTRAP_PERIOD
                    {
                        debug!("Running kademlia bootstrap procedure.");
                        let _ = self.swarm.behaviour_mut().kademlia.bootstrap();
                        kademlia_last_bootstrap = Instant::now();
                    }
                }
                _ = poll_closed(&mut self.bitswap_queries) => {
                    self.prune_canceled_bitswap_queries();
                }
                ev = self.swarm.select_next_some() => {
                    if let Err(e) = self.on_swarm_event(ev).await {
                        warn!("Failure while handling swarm event: {e}");
                    }
                },
                Some(cmd) = self.cmd_rx.recv() => {
                    if let Err(e) = self.on_cmd(cmd).await {
                        warn!("Failure while handling command. (error: {e})");
                    }
                }
            }
        }
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
            self.swarm.behaviour_mut().bitswap.cancel(query_id);
        }
    }

    async fn on_swarm_event(&mut self, ev: SwarmEvent<BehaviourEvent<B, S>>) -> Result<()> {
        match ev {
            SwarmEvent::Behaviour(ev) => match ev {
                BehaviourEvent::Identify(ev) => self.on_identify_event(ev).await?,
                BehaviourEvent::Gossipsub(ev) => self.on_gossip_sub_event(ev).await,
                BehaviourEvent::Kademlia(ev) => self.on_kademlia_event(ev).await?,
                BehaviourEvent::Bitswap(ev) => self.on_bitswap_event(ev).await,
                BehaviourEvent::Autonat(_)
                | BehaviourEvent::Ping(_)
                | BehaviourEvent::HeaderEx(_) => {}
            },
            SwarmEvent::ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint,
                ..
            } => {
                self.on_peer_connected(peer_id, connection_id, endpoint);
            }
            SwarmEvent::ConnectionClosed {
                peer_id,
                connection_id,
                ..
            } => {
                self.on_peer_disconnected(peer_id, connection_id);
            }
            _ => {}
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
                self.swarm
                    .behaviour_mut()
                    .header_ex
                    .send_request(request, respond_to);
            }
            P2pCmd::Listeners { respond_to } => {
                let local_peer_id = self.swarm.local_peer_id().to_owned();
                let listeners = self
                    .swarm
                    .listeners()
                    .cloned()
                    .map(|mut ma| {
                        if !ma.protocol_stack().any(|protocol| protocol == "p2p") {
                            ma.push(Protocol::P2p(local_peer_id))
                        }
                        ma
                    })
                    .collect();

                respond_to.maybe_send(listeners);
            }
            P2pCmd::ConnectedPeers { respond_to } => {
                respond_to.maybe_send(self.peer_tracker.connected_peers());
            }
            P2pCmd::InitHeaderSub { head } => {
                self.on_init_header_sub(*head);
            }
            P2pCmd::SetPeerTrust {
                peer_id,
                is_trusted,
            } => {
                if *self.swarm.local_peer_id() != peer_id {
                    self.peer_tracker.set_trusted(peer_id, is_trusted);
                }
            }
            P2pCmd::GetShwapCid { cid, respond_to } => {
                self.on_get_shwap_cid(cid, respond_to);
            }
            P2pCmd::GetNetworkCompromisedToken { respond_to } => {
                respond_to.maybe_send(self.network_compromised_token.child_token())
            }
        }

        Ok(())
    }

    #[instrument(skip_all)]
    fn report(&mut self) {
        let tracker_info = self.peer_tracker.info();

        info!(
            "peers: {}, trusted peers: {}",
            tracker_info.num_connected_peers, tracker_info.num_connected_trusted_peers,
        );
    }

    #[instrument(level = "trace", skip(self))]
    async fn on_identify_event(&mut self, ev: identify::Event) -> Result<()> {
        match ev {
            identify::Event::Received { peer_id, info } => {
                // Inform Kademlia about the listening addresses
                // TODO: Remove this when rust-libp2p#4302 is implemented
                for addr in info.listen_addrs {
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr);
                }
            }
            _ => trace!("Unhandled identify event"),
        }

        Ok(())
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
                    self.on_header_sub_message(&message.data[..]).await
                } else if message.topic == self.bad_encoding_fraud_sub_topic {
                    self.on_bad_encoding_fraud_sub_message(&message.data[..], &peer)
                        .await
                } else {
                    trace!("Unhandled gossipsub message");
                    gossipsub::MessageAcceptance::Ignore
                };

                if !matches!(acceptance, gossipsub::MessageAcceptance::Reject) {
                    // We may have discovered a new peer
                    self.peer_maybe_discovered(peer);
                }

                let _ = self
                    .swarm
                    .behaviour_mut()
                    .gossipsub
                    .report_message_validation_result(&message_id, &peer, acceptance);
            }
            _ => trace!("Unhandled gossipsub event"),
        }
    }

    #[instrument(level = "trace", skip(self))]
    async fn on_kademlia_event(&mut self, ev: kad::Event) -> Result<()> {
        match ev {
            kad::Event::RoutingUpdated {
                peer, addresses, ..
            } => {
                self.peer_tracker.add_addresses(peer, addresses.iter());
            }
            _ => trace!("Unhandled Kademlia event"),
        }

        Ok(())
    }

    #[instrument(level = "trace", skip_all)]
    fn on_get_shwap_cid(&mut self, cid: Cid, respond_to: OneshotResultSender<Vec<u8>, P2pError>) {
        trace!("Requesting CID {cid} from bitswap");
        let query_id = self.swarm.behaviour_mut().bitswap.get(&cid);
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

    #[instrument(skip_all, fields(peer_id = %peer_id))]
    fn peer_maybe_discovered(&mut self, peer_id: PeerId) {
        if !self.peer_tracker.set_maybe_discovered(peer_id) {
            return;
        }

        debug!("Peer discovered");
    }

    #[instrument(skip_all, fields(peer_id = %peer_id))]
    fn on_peer_connected(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        endpoint: ConnectedPoint,
    ) {
        debug!("Peer connected");

        // Inform PeerTracker about the dialed address.
        //
        // We do this because Kademlia send commands to Swarm
        // for dialing a peer and we may not have that address
        // in PeerTracker.
        let dialed_addr = match endpoint {
            ConnectedPoint::Dialer {
                address,
                role_override: Endpoint::Dialer,
            } => Some(address),
            _ => None,
        };

        self.peer_tracker
            .set_connected(peer_id, connection_id, dialed_addr);
    }

    #[instrument(skip_all, fields(peer_id = %peer_id))]
    fn on_peer_disconnected(&mut self, peer_id: PeerId, connection_id: ConnectionId) {
        if self
            .peer_tracker
            .set_maybe_disconnected(peer_id, connection_id)
        {
            debug!("Peer disconnected");
        }
    }

    #[instrument(skip_all, fields(header = %head))]
    fn on_init_header_sub(&mut self, head: ExtendedHeader) {
        self.header_sub_watcher.send_replace(Some(head));
        trace!("HeaderSub initialized");
    }

    #[instrument(skip_all)]
    async fn on_header_sub_message(&mut self, data: &[u8]) -> gossipsub::MessageAcceptance {
        let Ok(header) = ExtendedHeader::decode_and_validate(data) else {
            trace!("Malformed or invalid header from header-sub");
            return gossipsub::MessageAcceptance::Reject;
        };

        trace!("Received header from header-sub ({header})");

        let updated = self.header_sub_watcher.send_if_modified(move |state| {
            let Some(known_header) = state else {
                debug!("HeaderSub not initialized yet");
                return false;
            };

            if known_header.verify(&header).is_err() {
                trace!("Failed to verify HeaderSub header. Ignoring {header}");
                return false;
            }

            debug!("New header from header-sub ({header})");
            *state = Some(header);
            true
        });

        if updated {
            gossipsub::MessageAcceptance::Accept
        } else {
            gossipsub::MessageAcceptance::Ignore
        }
    }

    #[instrument(skip_all)]
    async fn on_bad_encoding_fraud_sub_message(
        &mut self,
        data: &[u8],
        peer: &PeerId,
    ) -> gossipsub::MessageAcceptance {
        let Ok(befp) = BadEncodingFraudProof::decode(data) else {
            trace!("Malformed bad encoding fraud proof from {peer}");
            self.swarm.behaviour_mut().gossipsub.blacklist_peer(peer);
            return gossipsub::MessageAcceptance::Reject;
        };

        let height = befp.height().value();
        let current_height =
            if let Some(network_height) = network_head_height(&self.header_sub_watcher) {
                network_height.value()
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
            self.swarm.behaviour_mut().gossipsub.blacklist_peer(peer);
            return gossipsub::MessageAcceptance::Reject;
        }

        warn!("Received a valid bad encoding fraud proof");
        // trigger cancellation for all services
        self.network_compromised_token.cancel();

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
        gossipsub.subscribe(topic)?;
    }

    Ok(gossipsub)
}

fn init_kademlia<B, S>(args: &P2pArgs<B, S>) -> Result<kad::Behaviour<kad::store::MemoryStore>>
where
    B: Blockstore,
    S: Store,
{
    let local_peer_id = PeerId::from(args.local_keypair.public());
    let mut config = kad::Config::default();

    let protocol_id = celestia_protocol_id(&args.network_id, "/kad/1.0.0");

    config.set_protocol_names(vec![protocol_id]);

    let store = kad::store::MemoryStore::new(local_peer_id);
    let mut kademlia = kad::Behaviour::with_config(local_peer_id, store, config);

    for addr in &args.bootnodes {
        if let Some(peer_id) = addr.peer_id() {
            kademlia.add_address(&peer_id, addr.to_owned());
        }
    }

    if !args.listen_on.is_empty() {
        kademlia.set_mode(Some(kad::Mode::Server));
    }

    Ok(kademlia)
}

fn init_bitswap<B, S>(
    blockstore: B,
    store: Arc<S>,
    network_id: &str,
) -> Result<beetswap::Behaviour<MAX_MH_SIZE, B>>
where
    B: Blockstore + 'static,
    S: Store + 'static,
{
    let protocol_prefix = format!("/celestia/{}", network_id);

    Ok(beetswap::Behaviour::builder(blockstore)
        .protocol_prefix(&protocol_prefix)?
        .register_multihasher(ShwapMultihasher::new(store))
        .client_set_send_dont_have(false)
        .build())
}

fn network_head_height(watcher: &watch::Sender<Option<ExtendedHeader>>) -> Option<Height> {
    watcher.borrow().as_ref().map(|header| header.height())
}
