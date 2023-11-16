use std::io;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use celestia_proto::p2p::pb::{header_request, HeaderRequest};
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use futures::StreamExt;
use instant::Instant;
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
use tokio::select;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{debug, info, instrument, trace, warn};

use crate::executor::spawn;
use crate::executor::Interval;
use crate::header_ex::{HeaderExBehaviour, HeaderExConfig};
use crate::peer_tracker::PeerTracker;
use crate::peer_tracker::PeerTrackerInfo;
use crate::store::Store;
use crate::swarm::new_swarm;
use crate::utils::{
    celestia_protocol_id, gossipsub_ident_topic, MultiaddrExt, OneshotResultSender,
    OneshotSenderExt,
};

pub use crate::header_ex::HeaderExError;

// Minimal number of peers that we want to maintain connection to.
// If we have fewer peers than that, we will try to reconnect / discover
// more aggresively.
const MIN_CONNECTED_PEERS: u64 = 4;
// Bootstrap procedure is a bit misleading as a name. It is actually
// scanning the network thought the already known peers and find new
// ones. It also recovers connectivity of previously known peers and
// refreshes the routing table.
//
// libp2p team suggests to start bootstrap procedure every 5 minute
const KADEMLIA_BOOTSTRAP_PERIOD: Duration = Duration::from_secs(5 * 60);

type Result<T, E = P2pError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum P2pError {
    #[error("Failed to initialize gossipsub behaviour: {0}")]
    GossipsubInit(String),

    #[error("Failed to on gossipsub subscribe: {0}")]
    GossipsubSubscribe(#[from] SubscriptionError),

    #[error("Transport error: {0}")]
    Transport(#[from] TransportError<io::Error>),

    #[error("Failed to initialize noise: {0}")]
    InitNoise(String),

    #[error("Dial error: {0}")]
    Dial(#[from] DialError),

    #[error("Worker died")]
    WorkerDied,

    #[error("Channel closed unexpectedly")]
    ChannelClosedUnexpectedly,

    #[error("Not connected to any peers")]
    NoConnectedPeers,

    #[error("HeaderEx: {0}")]
    HeaderEx(#[from] HeaderExError),

    #[error("Bootnode multiaddrs without peer ID: {0:?}")]
    BootnodeAddrsWithoutPeerId(Vec<Multiaddr>),
}

impl From<oneshot::error::RecvError> for P2pError {
    fn from(_value: oneshot::error::RecvError) -> Self {
        P2pError::ChannelClosedUnexpectedly
    }
}

#[derive(Debug)]
pub struct P2p<S>
where
    S: Store + 'static,
{
    cmd_tx: mpsc::Sender<P2pCmd>,
    header_sub_watcher: watch::Receiver<Option<ExtendedHeader>>,
    peer_tracker_info_watcher: watch::Receiver<PeerTrackerInfo>,
    local_peer_id: PeerId,
    _store: PhantomData<S>,
}

pub struct P2pArgs<S>
where
    S: Store + 'static,
{
    pub network_id: String,
    pub local_keypair: Keypair,
    pub bootnodes: Vec<Multiaddr>,
    pub listen_on: Vec<Multiaddr>,
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
}

impl<S> P2p<S>
where
    S: Store,
{
    pub fn start(args: P2pArgs<S>) -> Result<Self> {
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
            _store: PhantomData,
        })
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub fn mocked() -> (Self, crate::test_utils::MockP2pHandle) {
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (header_sub_tx, header_sub_rx) = watch::channel(None);
        let (peer_tracker_tx, peer_tracker_rx) = watch::channel(PeerTrackerInfo::default());

        let p2p = P2p {
            cmd_tx,
            header_sub_watcher: header_sub_rx,
            peer_tracker_info_watcher: peer_tracker_rx,
            local_peer_id: PeerId::random(),
            _store: PhantomData,
        };

        let handle = crate::test_utils::MockP2pHandle {
            cmd_rx,
            header_sub_tx,
            peer_tracker_tx,
        };

        (p2p, handle)
    }

    pub async fn stop(&self) -> Result<()> {
        // TODO
        Ok(())
    }

    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }

    async fn send_command(&self, cmd: P2pCmd) -> Result<()> {
        self.cmd_tx
            .send(cmd)
            .await
            .map_err(|_| P2pError::WorkerDied)
    }

    pub fn header_sub_watcher(&self) -> watch::Receiver<Option<ExtendedHeader>> {
        self.header_sub_watcher.clone()
    }

    pub fn peer_tracker_info_watcher(&self) -> watch::Receiver<PeerTrackerInfo> {
        self.peer_tracker_info_watcher.clone()
    }

    pub fn peer_tracker_info(&self) -> watch::Ref<PeerTrackerInfo> {
        self.peer_tracker_info_watcher.borrow()
    }

    pub async fn init_header_sub(&self, head: ExtendedHeader) -> Result<()> {
        self.send_command(P2pCmd::InitHeaderSub {
            head: Box::new(head),
        })
        .await
    }

    pub async fn wait_connected(&self) -> Result<()> {
        self.peer_tracker_info_watcher()
            .wait_for(|info| info.num_connected_peers > 0)
            .await
            .map(drop)
            .map_err(|_| P2pError::WorkerDied)
    }

    pub async fn wait_connected_trusted(&self) -> Result<()> {
        self.peer_tracker_info_watcher()
            .wait_for(|info| info.num_connected_trusted_peers > 0)
            .await
            .map(drop)
            .map_err(|_| P2pError::WorkerDied)
    }

    pub async fn network_info(&self) -> Result<NetworkInfo> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::NetworkInfo { respond_to: tx })
            .await?;

        Ok(rx.await?)
    }

    pub async fn header_ex_request(&self, request: HeaderRequest) -> Result<Vec<ExtendedHeader>> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::HeaderExRequest {
            request,
            respond_to: tx,
        })
        .await?;

        rx.await?
    }

    pub async fn get_head_header(&self) -> Result<ExtendedHeader> {
        self.get_header_by_height(0).await
    }

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

    pub async fn get_verified_headers_range(
        &self,
        from: &ExtendedHeader,
        amount: u64,
    ) -> Result<Vec<ExtendedHeader>> {
        from.validate().map_err(|_| HeaderExError::InvalidRequest)?;

        let height = from.height().value() + 1;

        let headers = self
            .header_ex_request(HeaderRequest {
                data: Some(header_request::Data::Origin(height)),
                amount,
            })
            .await?;

        from.verify_adjacent_range(&headers)
            .map_err(|_| HeaderExError::InvalidResponse)?;

        Ok(headers)
    }

    pub async fn listeners(&self) -> Result<Vec<Multiaddr>> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::Listeners { respond_to: tx })
            .await?;

        Ok(rx.await?)
    }

    pub async fn connected_peers(&self) -> Result<Vec<PeerId>> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::ConnectedPeers { respond_to: tx })
            .await?;

        Ok(rx.await?)
    }

    pub async fn set_peer_trust(&self, peer_id: PeerId, is_trusted: bool) -> Result<()> {
        self.send_command(P2pCmd::SetPeerTrust {
            peer_id,
            is_trusted,
        })
        .await
    }
}

/// Our network behaviour.
#[derive(NetworkBehaviour)]
struct Behaviour<S>
where
    S: Store + 'static,
{
    autonat: autonat::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    header_ex: HeaderExBehaviour<S>,
    gossipsub: gossipsub::Behaviour,
    kademlia: kad::Behaviour<kad::store::MemoryStore>,
}

struct Worker<S>
where
    S: Store + 'static,
{
    swarm: Swarm<Behaviour<S>>,
    header_sub_topic_hash: TopicHash,
    cmd_rx: mpsc::Receiver<P2pCmd>,
    peer_tracker: Arc<PeerTracker>,
    header_sub_watcher: watch::Sender<Option<ExtendedHeader>>,
}

impl<S> Worker<S>
where
    S: Store,
{
    fn new(
        args: P2pArgs<S>,
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
        let gossipsub = init_gossipsub(&args, [&header_sub_topic])?;

        let kademlia = init_kademlia(&args)?;

        let header_ex = HeaderExBehaviour::new(HeaderExConfig {
            network_id: &args.network_id,
            peer_tracker: peer_tracker.clone(),
            header_store: args.store.clone(),
        });

        let behaviour = Behaviour {
            autonat,
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
            header_sub_topic_hash: header_sub_topic.hash(),
            peer_tracker,
            header_sub_watcher,
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

    async fn on_swarm_event(&mut self, ev: SwarmEvent<BehaviourEvent<S>>) -> Result<()> {
        match ev {
            SwarmEvent::Behaviour(ev) => match ev {
                BehaviourEvent::Identify(ev) => self.on_identify_event(ev).await?,
                BehaviourEvent::Gossipsub(ev) => self.on_gossip_sub_event(ev).await,
                BehaviourEvent::Kademlia(ev) => self.on_kademlia_event(ev).await?,
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

                // We may discovered a new peer
                self.peer_maybe_discovered(peer);

                let acceptance = if message.topic == self.header_sub_topic_hash {
                    self.on_header_sub_message(&message.data[..]).await
                } else {
                    trace!("Unhandled gossipsub message");
                    gossipsub::MessageAcceptance::Ignore
                };

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

fn init_gossipsub<'a, S>(
    args: &'a P2pArgs<S>,
    topics: impl IntoIterator<Item = &'a gossipsub::IdentTopic>,
) -> Result<gossipsub::Behaviour>
where
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

fn init_kademlia<S>(args: &P2pArgs<S>) -> Result<kad::Behaviour<kad::store::MemoryStore>>
where
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
