use std::io;
use std::sync::Arc;

use celestia_types::ExtendedHeader;
use futures::channel::oneshot;
use futures::StreamExt;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::gossipsub::{self, SubscriptionError, TopicHash};
use libp2p::identity::Keypair;
use libp2p::swarm::{
    keep_alive, DialError, NetworkBehaviour, NetworkInfo, Swarm, SwarmBuilder, SwarmEvent,
    THandlerErr,
};
use libp2p::{identify, request_response, Multiaddr, PeerId, TransportError};
use log::{error, trace, warn};
use tendermint_proto::Protobuf;
use tokio::select;
use tokio::sync::RwLock;

use crate::exchange;
use crate::executor::{spawn, Executor};
use crate::store::Store;
use crate::utils::gossipsub_ident_topic;

/// Our network behaviour.
#[derive(NetworkBehaviour)]
struct Behaviour {
    identify: identify::Behaviour,
    header_ex: exchange::Behaviour,
    keep_alive: keep_alive::Behaviour,
    gossipsub: gossipsub::Behaviour,
}

pub struct P2p {
    cmd_tx: flume::Sender<P2pCmd>,
}

pub struct P2pConfig {
    pub transport: Boxed<(PeerId, StreamMuxerBox)>,
    pub store: Arc<RwLock<Store>>,
    pub network_id: String,
    pub local_keypair: Keypair,
    pub bootstrap_peers: Vec<Multiaddr>,
    pub listen_on: Vec<Multiaddr>,
}

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

    #[error("Command cancelled by worker: {0}")]
    CmdCancelled(String),

    #[error("Worker died")]
    WorkerDied,
}

type Result<T, E = P2pError> = std::result::Result<T, E>;

struct Worker {
    swarm: Swarm<Behaviour>,
    header_sub_topic_hash: TopicHash,
    cmd_rx: flume::Receiver<P2pCmd>,
}

#[derive(Debug)]
enum P2pCmd {
    NetworkInfo {
        respond_to: oneshot::Sender<NetworkInfo>,
    },
}

#[allow(unused)]
pub enum P2pEvent {}

impl P2p {
    pub fn new(config: P2pConfig) -> Result<P2p> {
        let local_peer_id = PeerId::from(config.local_keypair.public());

        let identify = identify::Behaviour::new(identify::Config::new(
            String::new(),
            config.local_keypair.public(),
        ));

        // Set the message authenticity - How we expect to publish messages
        // Here we expect the publisher to sign the message with their key.
        let message_authenticity =
            gossipsub::MessageAuthenticity::Signed(config.local_keypair.clone());
        // set default parameters for gossipsub
        let gossipsub_config = gossipsub::Config::default();
        // build a gossipsub network behaviour
        let mut gossipsub: gossipsub::Behaviour =
            gossipsub::Behaviour::new(message_authenticity, gossipsub_config)
                .map_err(P2pError::GossipsubInit)?;

        // subscribe to the topic
        let header_sub_topic = gossipsub_ident_topic(&config.network_id, "/header-sub/v0.0.1");
        gossipsub.subscribe(&header_sub_topic)?;

        let behaviour = Behaviour {
            identify,
            gossipsub,
            header_ex: exchange::new_behaviour(&config.network_id),
            keep_alive: keep_alive::Behaviour,
        };

        let mut swarm =
            SwarmBuilder::with_executor(config.transport, behaviour, local_peer_id, Executor)
                .build();

        for addr in config.listen_on {
            swarm.listen_on(addr)?;
        }

        for addr in config.bootstrap_peers {
            swarm.dial(addr)?;
        }

        let (cmd_tx, cmd_rx) = flume::bounded(16);

        spawn(async move {
            Worker {
                cmd_rx,
                swarm,
                header_sub_topic_hash: header_sub_topic.hash(),
            }
            .run()
            .await;
        });

        Ok(P2p { cmd_tx })
    }

    pub async fn next_event(&self) -> Option<P2pEvent> {
        todo!();
    }

    async fn send_cmd(&self, cmd: P2pCmd) -> Result<()> {
        self.cmd_tx
            .send_async(cmd)
            .await
            .map_err(|_| P2pError::WorkerDied)
    }

    pub async fn network_info(&self) -> Result<NetworkInfo> {
        let (tx, rx) = oneshot::channel();

        self.send_cmd(P2pCmd::NetworkInfo { respond_to: tx })
            .await?;
        rx.await
            .map_err(|_| P2pError::CmdCancelled("NetworkInfo".to_owned()))
    }
}

impl Worker {
    async fn run(&mut self) {
        let mut command_stream = self.cmd_rx.clone().into_stream().fuse();
        loop {
            select! {
                ev = self.swarm.select_next_some() => {
                    if let Err(e) = self.on_swarm_event(&ev).await {
                        warn!("Failure while handling swarm event. (error: {e}, event: {ev:?})");
                    }
                },
                Some(cmd) = command_stream.next() => {
                    if let Err(e) = self.on_command(cmd).await {
                        warn!("Failure while handling command. (error: {e})");
                    }
                }
            }
        }
    }

    async fn on_swarm_event(
        &mut self,
        ev: &SwarmEvent<BehaviourEvent, THandlerErr<Behaviour>>,
    ) -> Result<()> {
        trace!("{ev:?}");

        #[allow(clippy::single_match)]
        match ev {
            SwarmEvent::Behaviour(ev) => match ev {
                BehaviourEvent::Identify(ev) => self.on_identify_event(ev).await?,
                BehaviourEvent::HeaderEx(ev) => self.on_header_ex_event(ev).await?,
                BehaviourEvent::KeepAlive(_) => {}
                BehaviourEvent::Gossipsub(ev) => self.on_gossip_sub_event(ev).await?,
            },
            _ => {}
        }

        Ok(())
    }

    async fn on_command(&mut self, cmd: P2pCmd) -> Result<()> {
        trace!("{cmd:?}");

        match cmd {
            P2pCmd::NetworkInfo { respond_to } => {
                let _ = respond_to.send(self.swarm.network_info());
            }
        }

        Ok(())
    }

    async fn on_identify_event(&mut self, _ev: &identify::Event) -> Result<()> {
        // TODO
        Ok(())
    }

    async fn on_header_ex_event(&mut self, ev: &exchange::Event) -> Result<()> {
        match ev {
            request_response::Event::Message {
                peer,
                message:
                    request_response::Message::Response {
                        request_id,
                        response,
                    },
            } => {
                println!(
                    "Response for request: {request_id}, from peer: {peer}, status: {:?}",
                    response.status_code()
                );
                let header = ExtendedHeader::decode(&response.body[..]).unwrap();
                // TODO: Forward response back with one shot channel
                println!("Header: {header:?}");
            }
            _ => println!("Unhandled header_ex event: {ev:?}"),
        }

        Ok(())
    }

    async fn on_gossip_sub_event(&mut self, ev: &gossipsub::Event) -> Result<()> {
        match ev {
            gossipsub::Event::Message {
                message_id,
                message,
                ..
            } => {
                if message.topic == self.header_sub_topic_hash {
                    let header = ExtendedHeader::decode(&message.data[..]).unwrap();
                    // TODO: produce event

                    println!("New header from header-sub: {header:?}");
                } else {
                    println!("New gossipsub message, id: {message_id}, message: {message:?}");
                }
            }
            _ => println!("Unhandled gossipsub event: {ev:?}"),
        }

        Ok(())
    }
}
