use std::io;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use celestia_proto::p2p::pb::{header_request, HeaderRequest};
use celestia_types::hash::Hash;
use celestia_types::ExtendedHeader;
use futures::StreamExt;
use libp2p::{
    autonat,
    core::{muxing::StreamMuxerBox, transport::Boxed, ConnectedPoint, Endpoint},
    gossipsub::{self, SubscriptionError, TopicHash},
    identify,
    identity::Keypair,
    kad::{record::store::MemoryStore, Kademlia, KademliaConfig, KademliaEvent, Mode},
    multiaddr::Protocol,
    ping,
    swarm::{
        ConnectionId, DialError, NetworkBehaviour, NetworkInfo, Swarm, SwarmBuilder, SwarmEvent,
        THandlerErr,
    },
    Multiaddr, PeerId, TransportError,
};
use tokio::select;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{debug, info, instrument, trace, warn};

use crate::exchange::{ExchangeBehaviour, ExchangeConfig};
use crate::executor::Interval;
use crate::executor::{spawn, Executor};
use crate::peer_tracker::PeerTracker;
use crate::peer_tracker::PeerTrackerInfo;
use crate::store::Store;
use crate::utils::{
    celestia_protocol_id, gossipsub_ident_topic, MultiaddrExt, OneshotResultSender,
    OneshotSenderExt,
};

pub use crate::exchange::ExchangeError;

type Result<T, E = P2pError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum P2pError {
    #[error("Failed to initialize gossipsub behaviour: {0}")]
    GossipsubInit(&'static str),

    #[error("Failed to on gossipsub subscribe: {0}")]
    GossipsubSubscribe(#[from] SubscriptionError),

    #[error("Transport error: {0}")]
    Transport(#[from] TransportError<io::Error>),

    #[error("Dial error: {0}")]
    Dial(#[from] DialError),

    #[error("Worker died")]
    WorkerDied,

    #[error("Channel closed unexpectedly")]
    ChannelClosedUnexpectedly,

    #[error("Not connected to any peers")]
    NoConnectedPeers,

    #[error("Exchange: {0}")]
    Exchange(#[from] ExchangeError),

    #[error("HeaderSub already initialized")]
    HeaderSubAlreadyInitialized,
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
    _store: PhantomData<S>,
}

pub struct P2pArgs<S>
where
    S: Store + 'static,
{
    pub transport: Boxed<(PeerId, StreamMuxerBox)>,
    pub network_id: String,
    pub local_keypair: Keypair,
    pub bootstrap_peers: Vec<Multiaddr>,
    pub listen_on: Vec<Multiaddr>,
    pub store: Arc<S>,
}

#[derive(Debug)]
enum P2pCmd {
    NetworkInfo {
        respond_to: oneshot::Sender<NetworkInfo>,
    },
    ExchangeHeaderRequest {
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
        respond_to: OneshotResultSender<(), P2pError>,
    },
}

impl<S> P2p<S>
where
    S: Store,
{
    pub async fn start(args: P2pArgs<S>) -> Result<Self, P2pError> {
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
            _store: PhantomData,
        })
    }

    pub async fn stop(&self) -> Result<()> {
        // TODO
        Ok(())
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

    pub async fn init_header_sub(&self, head: ExtendedHeader) -> Result<()> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::InitHeaderSub {
            head: Box::new(head),
            respond_to: tx,
        })
        .await?;

        rx.await?
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

    pub async fn exchange_header_request(
        &self,
        request: HeaderRequest,
    ) -> Result<Vec<ExtendedHeader>> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::ExchangeHeaderRequest {
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
        self.exchange_header_request(HeaderRequest {
            data: Some(header_request::Data::Hash(hash.as_bytes().to_vec())),
            amount: 1,
        })
        .await?
        .into_iter()
        .next()
        .ok_or(ExchangeError::HeaderNotFound.into())
    }

    pub async fn get_header_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        self.exchange_header_request(HeaderRequest {
            data: Some(header_request::Data::Origin(height)),
            amount: 1,
        })
        .await?
        .into_iter()
        .next()
        .ok_or(ExchangeError::HeaderNotFound.into())
    }

    pub async fn get_verified_headers_range(
        &self,
        from: &ExtendedHeader,
        amount: u64,
    ) -> Result<Vec<ExtendedHeader>> {
        from.validate().map_err(|_| ExchangeError::InvalidRequest)?;

        let height = from.height().value() + 1;

        let headers = self
            .exchange_header_request(HeaderRequest {
                data: Some(header_request::Data::Origin(height)),
                amount,
            })
            .await?;

        from.verify_adjacent_range(&headers)
            .map_err(|_| ExchangeError::InvalidResponse)?;

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
    header_ex: ExchangeBehaviour<S>,
    gossipsub: gossipsub::Behaviour,
    kademlia: Kademlia<MemoryStore>,
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

        let header_ex = ExchangeBehaviour::new(ExchangeConfig {
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

        let mut swarm =
            SwarmBuilder::with_executor(args.transport, behaviour, local_peer_id, Executor).build();

        for addr in args.listen_on {
            swarm.listen_on(addr)?;
        }

        for addr in args.bootstrap_peers {
            // Bootstrap peers are always trusted
            if let Some(peer_id) = addr.peer_id() {
                peer_tracker.set_trusted(peer_id);
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
        let mut interval = Interval::new(Duration::from_secs(60)).await;
        let mut first_report = false;

        loop {
            if !first_report {
                if self.peer_tracker.info().num_connected_peers > 0 {
                    self.report();
                    first_report = true;
                }
            }

            select! {
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
                _ = interval.tick() => {
                    self.report();
                }
            }
        }
    }

    async fn on_swarm_event(
        &mut self,
        ev: SwarmEvent<BehaviourEvent<S>, THandlerErr<Behaviour<S>>>,
    ) -> Result<()> {
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
            P2pCmd::ExchangeHeaderRequest {
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
            P2pCmd::InitHeaderSub { head, respond_to } => {
                let res = self.on_init_header_sub(*head).await;
                respond_to.maybe_send(res);
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
                let kademlia = &mut self.swarm.behaviour_mut().kademlia;

                // Inform peer tracker
                self.peer_tracker.set_identified(peer_id, &info);

                // Inform Kademlia
                for addr in info.listen_addrs {
                    kademlia.add_address(&peer_id, addr);
                }

                // Start a deeper lookup for other peers
                kademlia.get_closest_peers(peer_id);
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
    async fn on_kademlia_event(&mut self, ev: KademliaEvent) -> Result<()> {
        match ev {
            KademliaEvent::RoutingUpdated {
                peer, addresses, ..
            } => {
                self.peer_tracker.add_addresses(peer, addresses.iter());
            }
            KademliaEvent::UnroutablePeer { peer } => {
                // Kademlia does know the address of the peer, but we might have
                // it in PeerTracker
                for addr in self.peer_tracker.addresses(peer) {
                    self.swarm.behaviour_mut().kademlia.add_address(&peer, addr);
                }
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

        // Initiate deeper lookup
        self.swarm
            .behaviour_mut()
            .kademlia
            .get_closest_peers(peer_id);
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

    #[instrument(skip_all)]
    async fn on_init_header_sub(&mut self, head: ExtendedHeader) -> Result<()> {
        let updated = self.header_sub_watcher.send_if_modified(move |state| {
            if state.is_none() {
                *state = Some(head);
                true
            } else {
                false
            }
        });

        if updated {
            trace!("HeaderSub initialized");
            Ok(())
        } else {
            Err(P2pError::HeaderSubAlreadyInitialized)
        }
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
        .map_err(P2pError::GossipsubInit)?;

    // build a gossipsub network behaviour
    let mut gossipsub: gossipsub::Behaviour =
        gossipsub::Behaviour::new(message_authenticity, config).map_err(P2pError::GossipsubInit)?;

    for topic in topics {
        gossipsub.subscribe(topic)?;
    }

    Ok(gossipsub)
}

fn init_kademlia<S>(args: &P2pArgs<S>) -> Result<Kademlia<MemoryStore>>
where
    S: Store,
{
    let local_peer_id = PeerId::from(args.local_keypair.public());
    let mut config = KademliaConfig::default();

    let protocol_id = celestia_protocol_id(&args.network_id, "/kad/1.0.0");

    config.set_protocol_names(vec![protocol_id]);

    let mut kad = Kademlia::with_config(local_peer_id, MemoryStore::new(local_peer_id), config);

    for addr in &args.bootstrap_peers {
        if let Some(peer_id) = addr.peer_id() {
            kad.add_address(&peer_id, addr.to_owned());
        }
    }

    if !args.listen_on.is_empty() {
        kad.set_mode(Some(Mode::Server));
    }

    // Peer multiaddress may not contain the peer. This is not fatal
    // because after we join to a bootstrap peer we initiate `get_closest_peers`.
    if let Err(e) = kad.bootstrap() {
        warn!("Failed to start Kademlia boostrap: {e}");
    }

    Ok(kad)
}
