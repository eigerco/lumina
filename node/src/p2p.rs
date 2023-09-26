use std::io;
use std::marker::PhantomData;
use std::sync::Arc;

use async_trait::async_trait;
use celestia_proto::p2p::pb::{header_request, HeaderRequest};
use celestia_types::{ExtendedHeader, Hash};
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
        keep_alive, DialError, NetworkBehaviour, NetworkInfo, Swarm, SwarmBuilder, SwarmEvent,
        THandlerErr,
    },
    Multiaddr, PeerId, TransportError,
};
use tendermint_proto::Protobuf;
use tokio::select;
use tokio::sync::oneshot;
use tracing::{debug, info, instrument, trace, warn};

use crate::exchange::{ExchangeBehaviour, ExchangeConfig};
use crate::executor::{spawn, Executor};
use crate::peer_tracker::PeerTracker;
use crate::store::Store;
use crate::utils::{
    celestia_protocol_id, gossipsub_ident_topic, MultiaddrExt, OneshotResultSender,
    OneshotSenderExt,
};
use crate::Service;

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
}

impl From<oneshot::error::RecvError> for P2pError {
    fn from(_value: oneshot::error::RecvError) -> Self {
        P2pError::ChannelClosedUnexpectedly
    }
}

#[derive(Debug)]
pub struct P2p<S> {
    cmd_tx: flume::Sender<P2pCmd>,
    _store: PhantomData<S>,
}

pub struct P2pArgs<S> {
    pub transport: Boxed<(PeerId, StreamMuxerBox)>,
    pub network_id: String,
    pub local_keypair: Keypair,
    pub bootstrap_peers: Vec<Multiaddr>,
    pub listen_on: Vec<Multiaddr>,
    pub store: Arc<S>,
}

#[doc(hidden)]
#[derive(Debug)]
pub enum P2pCmd {
    NetworkInfo {
        respond_to: oneshot::Sender<NetworkInfo>,
    },
    ExchangeHeaderRequest {
        request: HeaderRequest,
        respond_to: OneshotResultSender<Vec<ExtendedHeader>, P2pError>,
    },
    WaitConnected {
        respond_to: oneshot::Sender<()>,
    },
    Listeners {
        respond_to: oneshot::Sender<Vec<Multiaddr>>,
    },
    ConnectedPeers {
        respond_to: oneshot::Sender<Vec<PeerId>>,
    },
}

#[async_trait]
impl<S> Service for P2p<S>
where
    S: Store + 'static,
{
    type Command = P2pCmd;
    type Args = P2pArgs<S>;
    type Error = P2pError;

    async fn start(args: P2pArgs<S>) -> Result<Self, P2pError> {
        let (cmd_tx, cmd_rx) = flume::bounded(16);
        let mut worker = Worker::new(args, cmd_rx)?;

        spawn(async move {
            worker.run().await;
        });

        Ok(P2p {
            cmd_tx,
            _store: PhantomData,
        })
    }

    async fn stop(&self) -> Result<()> {
        // TODO
        Ok(())
    }

    async fn send_command(&self, cmd: P2pCmd) -> Result<()> {
        self.cmd_tx
            .send_async(cmd)
            .await
            .map_err(|_| P2pError::WorkerDied)
    }
}

#[async_trait]
pub trait P2pService:
    Service<Args = P2pArgs<Self::Store>, Command = P2pCmd, Error = P2pError>
{
    type Store: Store;

    async fn wait_connected(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::WaitConnected { respond_to: tx })
            .await?;

        Ok(rx.await?)
    }

    async fn network_info(&self) -> Result<NetworkInfo> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::NetworkInfo { respond_to: tx })
            .await?;

        Ok(rx.await?)
    }

    async fn exchange_header_request(&self, request: HeaderRequest) -> Result<Vec<ExtendedHeader>> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::ExchangeHeaderRequest {
            request,
            respond_to: tx,
        })
        .await?;

        rx.await?
    }

    async fn get_head_header(&self) -> Result<ExtendedHeader> {
        self.get_header_by_height(0).await
    }

    async fn get_header(&self, hash: Hash) -> Result<ExtendedHeader> {
        self.exchange_header_request(HeaderRequest {
            data: Some(header_request::Data::Hash(hash.as_bytes().to_vec())),
            amount: 1,
        })
        .await?
        .into_iter()
        .next()
        .ok_or(ExchangeError::HeaderNotFound.into())
    }

    async fn get_header_by_height(&self, height: u64) -> Result<ExtendedHeader> {
        self.exchange_header_request(HeaderRequest {
            data: Some(header_request::Data::Origin(height)),
            amount: 1,
        })
        .await?
        .into_iter()
        .next()
        .ok_or(ExchangeError::HeaderNotFound.into())
    }

    async fn get_headers_range(&self, height: u64, amount: u64) -> Result<Vec<ExtendedHeader>> {
        self.exchange_header_request(HeaderRequest {
            data: Some(header_request::Data::Origin(height)),
            amount,
        })
        .await
    }

    async fn get_verified_headers_range(
        &self,
        from: &ExtendedHeader,
        amount: u64,
    ) -> Result<Vec<ExtendedHeader>> {
        from.validate().map_err(|_| ExchangeError::InvalidRequest)?;

        let headers = self
            .get_headers_range(from.height().value() + 1, amount)
            .await?;

        for untrusted in headers.iter() {
            untrusted
                .validate()
                .map_err(|_| ExchangeError::InvalidResponse)?;

            from.verify(untrusted)
                .map_err(|_| ExchangeError::InvalidResponse)?;
        }

        Ok(headers)
    }

    async fn listeners(&self) -> Result<Vec<Multiaddr>> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::Listeners { respond_to: tx })
            .await?;

        Ok(rx.await?)
    }

    async fn connected_peers(&self) -> Result<Vec<PeerId>> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::ConnectedPeers { respond_to: tx })
            .await?;

        Ok(rx.await?)
    }
}

#[async_trait]
impl<S> P2pService for P2p<S>
where
    S: Store + 'static,
{
    type Store = S;
}

/// Our network behaviour.
#[derive(NetworkBehaviour)]
struct Behaviour<S>
where
    S: Store + 'static,
{
    autonat: autonat::Behaviour,
    keep_alive: keep_alive::Behaviour,
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
    cmd_rx: flume::Receiver<P2pCmd>,
    peer_tracker: Arc<PeerTracker>,
    wait_connected_tx: Option<Vec<oneshot::Sender<()>>>,
}

impl<S> Worker<S>
where
    S: Store + 'static,
{
    fn new(args: P2pArgs<S>, cmd_rx: flume::Receiver<P2pCmd>) -> Result<Self, P2pError> {
        let peer_tracker = Arc::new(PeerTracker::new());
        let local_peer_id = PeerId::from(args.local_keypair.public());

        let autonat = autonat::Behaviour::new(local_peer_id, autonat::Config::default());
        let keep_alive = keep_alive::Behaviour;
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
            header_store: args.store,
        });

        let behaviour = Behaviour {
            autonat,
            keep_alive,
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
            swarm.dial(addr)?;
        }

        Ok(Worker {
            cmd_rx,
            swarm,
            header_sub_topic_hash: header_sub_topic.hash(),
            peer_tracker,
            wait_connected_tx: None,
        })
    }

    async fn run(&mut self) {
        let mut cmd_stream = self.cmd_rx.clone().into_stream().fuse();

        loop {
            select! {
                ev = self.swarm.select_next_some() => {
                    if let Err(e) = self.on_swarm_event(ev).await {
                        warn!("Failure while handling swarm event: {e}");
                    }
                },
                Some(cmd) = cmd_stream.next() => {
                    if let Err(e) = self.on_cmd(cmd).await {
                        warn!("Failure while handling command. (error: {e})");
                    }
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
                BehaviourEvent::Gossipsub(ev) => self.on_gossip_sub_event(ev).await?,
                BehaviourEvent::Kademlia(ev) => self.on_kademlia_event(ev).await?,
                BehaviourEvent::Autonat(_)
                | BehaviourEvent::KeepAlive(_)
                | BehaviourEvent::Ping(_)
                | BehaviourEvent::HeaderEx(_) => {}
            },
            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                self.on_peer_connected(peer_id, endpoint);
            }
            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                self.on_peer_disconnected(peer_id);
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
            P2pCmd::WaitConnected { respond_to } => {
                self.on_wait_connected(respond_to);
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
        }

        Ok(())
    }

    #[instrument(level = "trace", skip(self))]
    async fn on_identify_event(&mut self, ev: identify::Event) -> Result<()> {
        match ev {
            identify::Event::Received { peer_id, info } => {
                let kademlia = &mut self.swarm.behaviour_mut().kademlia;

                // Inform peer tracker
                self.peer_tracker.identified(peer_id, &info);

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
    async fn on_gossip_sub_event(&mut self, ev: gossipsub::Event) -> Result<()> {
        match ev {
            gossipsub::Event::Message { message, .. } => {
                // We may discovered a new peer
                if let Some(peer) = message.source {
                    self.on_peer_discovered(peer);
                }

                if message.topic == self.header_sub_topic_hash {
                    self.on_header_sub_message(&message.data[..]);
                } else {
                    trace!("Unhandled gossipsub message");
                }
            }
            _ => trace!("Unhandled gossipsub event"),
        }

        Ok(())
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
    fn on_peer_discovered(&mut self, peer_id: PeerId) {
        if !self.peer_tracker.maybe_discovered(peer_id) {
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
    fn on_peer_connected(&mut self, peer_id: PeerId, endpoint: ConnectedPoint) {
        info!("Peer connected");

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

        self.peer_tracker.connected(peer_id, dialed_addr);

        for tx in self.wait_connected_tx.take().into_iter().flatten() {
            tx.maybe_send(());
        }
    }

    #[instrument(skip_all, fields(peer_id = %peer_id))]
    fn on_peer_disconnected(&mut self, peer_id: PeerId) {
        info!("Peer disconnected");
        self.peer_tracker.disconnected(peer_id);
    }

    fn on_wait_connected(&mut self, respond_to: oneshot::Sender<()>) {
        if self.peer_tracker.is_anyone_connected() {
            respond_to.maybe_send(());
        } else {
            self.wait_connected_tx
                .get_or_insert_with(Vec::new)
                .push(respond_to);
        }
    }

    #[instrument(skip_all)]
    fn on_header_sub_message(&mut self, data: &[u8]) {
        let Ok(header) = ExtendedHeader::decode(data) else {
            trace!("Malformed header from header-sub");
            return;
        };

        if let Err(e) = header.validate() {
            trace!("Invalid header from header-sub ({e})");
            return;
        }

        debug!("New header from header-sub ({header})");
        // TODO: inform syncer about it
        //
        // TODO: Verify header against store. If it's verified propagate
        // it to other peers. If not, discard it.
    }
}

fn init_gossipsub<'a, S>(
    args: &'a P2pArgs<S>,
    topics: impl IntoIterator<Item = &'a gossipsub::IdentTopic>,
) -> Result<gossipsub::Behaviour> {
    // Set the message authenticity - How we expect to publish messages
    // Here we expect the publisher to sign the message with their key.
    let message_authenticity = gossipsub::MessageAuthenticity::Signed(args.local_keypair.clone());

    // build a gossipsub network behaviour
    let mut gossipsub: gossipsub::Behaviour =
        gossipsub::Behaviour::new(message_authenticity, gossipsub::Config::default())
            .map_err(P2pError::GossipsubInit)?;

    for topic in topics {
        gossipsub.subscribe(topic)?;
    }

    Ok(gossipsub)
}

fn init_kademlia<S>(args: &P2pArgs<S>) -> Result<Kademlia<MemoryStore>> {
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
