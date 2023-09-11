use std::io;
use std::sync::Arc;

use async_trait::async_trait;
use celestia_proto::p2p::pb::HeaderRequest;
use celestia_types::ExtendedHeader;
use futures::StreamExt;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::gossipsub::{self, SubscriptionError, TopicHash};
use libp2p::identity::Keypair;
use libp2p::swarm::{
    keep_alive, DialError, NetworkBehaviour, NetworkInfo, Swarm, SwarmBuilder, SwarmEvent,
    THandlerErr,
};
use libp2p::{identify, Multiaddr, PeerId, TransportError};
use tendermint_proto::Protobuf;
use tokio::select;
use tokio::sync::oneshot;
use tracing::{debug, instrument, warn};

use crate::exchange::{ExchangeBehaviour, ExchangeConfig, ExchangeError};
use crate::executor::{spawn, Executor};
use crate::peer_book::PeerBook;
use crate::utils::{gossipsub_ident_topic, OneshotSenderExt};
use crate::Service;

type Result<T, E = P2pError> = std::result::Result<T, E>;
type OneshotResultSender<T, E = P2pError> = oneshot::Sender<Result<T, E>>;

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

    #[error("Header not found")]
    HeaderNotFound,

    #[error("Unsupported header response")]
    UnsupportedHeaderResponse,
}

#[derive(Debug)]
pub struct P2p {
    cmd_tx: flume::Sender<P2pCmd>,
}

pub struct P2pArgs {
    pub transport: Boxed<(PeerId, StreamMuxerBox)>,
    pub network_id: String,
    pub local_keypair: Keypair,
    pub bootstrap_peers: Vec<Multiaddr>,
    pub listen_on: Vec<Multiaddr>,
}

#[doc(hidden)]
#[derive(Debug)]
pub enum P2pCmd {
    NetworkInfo {
        respond_to: oneshot::Sender<NetworkInfo>,
    },
    ExchangeHeaderRequest {
        request: HeaderRequest,
        respond_to: OneshotResultSender<ExtendedHeader, ExchangeError>,
    },
}

#[async_trait]
impl Service for P2p {
    type Command = P2pCmd;
    type Args = P2pArgs;
    type Error = P2pError;

    async fn start(args: P2pArgs) -> Result<Self, P2pError> {
        let (cmd_tx, cmd_rx) = flume::bounded(16);
        let mut worker = Worker::new(args, cmd_rx)?;

        spawn(async move {
            worker.run().await;
        });

        Ok(P2p { cmd_tx })
    }

    async fn stop(&self) -> Result<()> {
        todo!()
    }

    async fn send_command(&self, cmd: P2pCmd) -> Result<()> {
        self.cmd_tx
            .send_async(cmd)
            .await
            .map_err(|_| P2pError::WorkerDied)
    }
}

#[async_trait]
pub trait P2pService: Service<Args = P2pArgs, Command = P2pCmd, Error = P2pError> {
    async fn network_info(&self) -> Result<NetworkInfo> {
        let (tx, rx) = oneshot::channel();

        self.send_command(P2pCmd::NetworkInfo { respond_to: tx })
            .await?;
        rx.await.map_err(|_| P2pError::WorkerDied)
    }
}

#[async_trait]
impl P2pService for P2p {}

/// Our network behaviour.
#[derive(NetworkBehaviour)]
struct Behaviour {
    identify: identify::Behaviour,
    header_ex: ExchangeBehaviour,
    keep_alive: keep_alive::Behaviour,
    gossipsub: gossipsub::Behaviour,
}

struct Worker {
    swarm: Swarm<Behaviour>,
    header_sub_topic_hash: TopicHash,
    cmd_rx: flume::Receiver<P2pCmd>,
    peer_book: Arc<PeerBook>,
}

impl Worker {
    fn new(args: P2pArgs, cmd_rx: flume::Receiver<P2pCmd>) -> Result<Self, P2pError> {
        let peer_book = Arc::new(PeerBook::new());
        let local_peer_id = PeerId::from(args.local_keypair.public());

        let identify = identify::Behaviour::new(identify::Config::new(
            String::new(),
            args.local_keypair.public(),
        ));

        // Set the message authenticity - How we expect to publish messages
        // Here we expect the publisher to sign the message with their key.
        let message_authenticity =
            gossipsub::MessageAuthenticity::Signed(args.local_keypair.clone());
        // set default parameters for gossipsub
        let gossipsub_config = gossipsub::Config::default();
        // build a gossipsub network behaviour
        let mut gossipsub: gossipsub::Behaviour =
            gossipsub::Behaviour::new(message_authenticity, gossipsub_config)
                .map_err(P2pError::GossipsubInit)?;

        // subscribe to the topic
        let header_sub_topic = gossipsub_ident_topic(&args.network_id, "/header-sub/v0.0.1");
        gossipsub.subscribe(&header_sub_topic)?;

        let header_ex = ExchangeBehaviour::new(ExchangeConfig {
            network_id: &args.network_id,
            peer_book: peer_book.clone(),
        });

        let behaviour = Behaviour {
            identify,
            gossipsub,
            header_ex,
            keep_alive: keep_alive::Behaviour,
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
            peer_book,
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

    #[instrument(level = "trace", skip(self))]
    async fn on_swarm_event(
        &mut self,
        ev: SwarmEvent<BehaviourEvent, THandlerErr<Behaviour>>,
    ) -> Result<()> {
        match ev {
            SwarmEvent::Behaviour(ev) => match ev {
                BehaviourEvent::Identify(ev) => self.on_identify_event(ev).await?,
                BehaviourEvent::Gossipsub(ev) => self.on_gossip_sub_event(ev).await?,
                BehaviourEvent::HeaderEx(_) | BehaviourEvent::KeepAlive(_) => {}
            },
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                self.peer_book.add(peer_id);
            }
            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                self.peer_book.remove(peer_id);
            }
            _ => {}
        }

        Ok(())
    }

    #[instrument(level = "trace", skip(self))]
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
        }

        Ok(())
    }

    async fn on_identify_event(&mut self, _ev: identify::Event) -> Result<()> {
        // TODO
        Ok(())
    }

    async fn on_gossip_sub_event(&mut self, ev: gossipsub::Event) -> Result<()> {
        match ev {
            gossipsub::Event::Message {
                message_id,
                message,
                ..
            } => {
                if message.topic == self.header_sub_topic_hash {
                    let header = ExtendedHeader::decode(&message.data[..]).unwrap();
                    // TODO: produce event

                    debug!("New header from header-sub: {header:?}");
                } else {
                    debug!("New gossipsub message, id: {message_id}, message: {message:?}");
                }
            }
            _ => debug!("Unhandled gossipsub event: {ev:?}"),
        }

        Ok(())
    }
}
