use std::io;
use std::sync::Arc;
use std::task::{Context, Poll};

use async_trait::async_trait;
use either::Either;
use futures::{AsyncRead, AsyncWrite};
use libp2p::core::Endpoint;
use libp2p::core::transport::PortUse;
use libp2p::gossipsub::{self, Message};
use libp2p::identity::Keypair;
use libp2p::request_response::{self, Codec, ProtocolSupport};
use libp2p::swarm::handler::ConnectionEvent;
use libp2p::swarm::{
    ConnectionDenied, ConnectionHandler, ConnectionHandlerEvent, ConnectionHandlerSelect,
    ConnectionId, FromSwarm, NetworkBehaviour, SubstreamProtocol, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};
use libp2p::{Multiaddr, PeerId, StreamProtocol};
use tracing::{debug, info};

use crate::p2p::P2pError;
use crate::p2p::shrex::pool_tracker::EdsNotification;
use crate::store::Store;
use crate::utils::protocol_id;

pub(crate) mod pool_tracker;

pub(crate) struct Config<'a, S> {
    pub network_id: &'a str,
    pub local_keypair: &'a Keypair,
    pub header_store: Arc<S>,
}

pub(crate) struct Behaviour<S>
where
    S: Store + 'static,
{
    req_resp: request_response::Behaviour<TodoCodec>,
    // TODO: this is a workaround, should be replaced with a real floodsub
    // rust-libp2p implementation of the floodsub isn't compliant with a spec,
    // so we work that around by using a gossipsub configured with floodsub support.
    // Gossipsub will always receive from and forward messages to all the floodsub peers.
    // Since we always maintain a connection with a few bridge (and possibly archival)
    // nodes, then if we assume those nodes correctly use only floodsub protocol,
    // we cannot be isolated in a way described in a shrex-sub spec:
    // https://github.com/celestiaorg/celestia-node/blob/76db37cc4ac09e892122a081b8bea24f87899f11/specs/src/shrex/shrex-sub.md#why-not-gossipsub
    shrex_sub: gossipsub::Behaviour,
    pool_tracker: pool_tracker::PoolTracker<S>,
    _store: Arc<S>,
}

#[derive(Debug)]
pub(crate) enum Event {}

impl<S> Behaviour<S>
where
    S: Store,
{
    pub fn new(config: Config<'_, S>) -> Result<Self, P2pError> {
        let message_authenticity =
            gossipsub::MessageAuthenticity::Signed(config.local_keypair.clone());

        let shrex_sub_config = gossipsub::ConfigBuilder::default()
            // replace default meshsub protocols with something that won't match
            // note: this may create an additional, exclusive lumina-only gossip mesh :)
            .protocol_id_prefix("/nosub")
            // add floodsub protocol
            .support_floodsub()
            // and floodsub publish behaviour
            .flood_publish(true)
            .validation_mode(gossipsub::ValidationMode::Strict)
            .validate_messages()
            .build()
            .map_err(|e| P2pError::GossipsubInit(e.to_string()))?;

        // build a gossipsub network behaviour
        let mut shrex_sub: gossipsub::Behaviour =
            gossipsub::Behaviour::new(message_authenticity, shrex_sub_config)
                .map_err(|e| P2pError::GossipsubInit(e.to_string()))?;

        let topic = format!("{}/eds-sub/v0.2.0", config.network_id);
        shrex_sub
            .subscribe(&gossipsub::IdentTopic::new(topic))
            .map_err(|e| P2pError::GossipsubInit(e.to_string()))?;

        Ok(Self {
            shrex_sub,
            req_resp: request_response::Behaviour::new(
                [(
                    protocol_id(config.network_id, "/todo/v0.0.1"),
                    ProtocolSupport::Full,
                )],
                request_response::Config::default(),
            ),
            pool_tracker: pool_tracker::PoolTracker::new(config.header_store.clone()),
            _store: config.header_store,
        })
    }

    fn handle_shrex_event(
        &mut self,
        ev: ToSwarm<gossipsub::Event, THandlerInEvent<gossipsub::Behaviour>>,
    ) -> Poll<ToSwarm<Event, THandlerInEvent<Self>>> {
        if let ToSwarm::GenerateEvent(ev) = ev {
            match ev {
                gossipsub::Event::Message {
                    message: Message { source, data, .. },
                    ..
                } => {
                    if let Ok(EdsNotification { height, data_hash }) =
                        EdsNotification::deserialize_and_validate(data.as_ref())
                    {
                        if let Some(peer_id) = source {
                            self.pool_tracker
                                .add_peer_for_hash(peer_id, data_hash, height);
                        }
                    } else {
                        debug!("Invalid shrex sub message");
                    }
                }
                gossipsub::Event::SlowPeer {
                    peer_id,
                    failed_messages,
                } => info!("{peer_id} slow: {failed_messages:?}"),
                _ => (),
            }
        } else {
            return Poll::Ready(
                ev.map_out(|_| unreachable!("GenerateEvent handled"))
                    .map_in(Either::Left),
            );
        }
        Poll::Pending
    }
}

impl<S> NetworkBehaviour for Behaviour<S>
where
    S: Store + 'static,
{
    type ConnectionHandler = ConnHandler;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<Self::ConnectionHandler, ConnectionDenied> {
        Ok(ConnHandler(ConnectionHandler::select(
            self.shrex_sub.handle_established_inbound_connection(
                connection_id,
                peer,
                local_addr,
                remote_addr,
            )?,
            self.req_resp.handle_established_inbound_connection(
                connection_id,
                peer,
                local_addr,
                remote_addr,
            )?,
        )))
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
        port_use: PortUse,
    ) -> Result<Self::ConnectionHandler, ConnectionDenied> {
        Ok(ConnHandler(ConnectionHandler::select(
            self.shrex_sub.handle_established_outbound_connection(
                connection_id,
                peer,
                addr,
                role_override,
                port_use,
            )?,
            self.req_resp.handle_established_outbound_connection(
                connection_id,
                peer,
                addr,
                role_override,
                port_use,
            )?,
        )))
    }

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.shrex_sub
            .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)?;
        self.req_resp
            .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)?;
        Ok(())
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        let mut combined_addresses = Vec::new();

        combined_addresses.extend(self.shrex_sub.handle_pending_outbound_connection(
            connection_id,
            maybe_peer,
            addresses,
            effective_role,
        )?);
        combined_addresses.extend(self.req_resp.handle_pending_outbound_connection(
            connection_id,
            maybe_peer,
            addresses,
            effective_role,
        )?);

        Ok(combined_addresses)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        self.shrex_sub.on_swarm_event(event);
        self.req_resp.on_swarm_event(event);
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        match event {
            Either::Left(shrex_sub_ev) => {
                self.shrex_sub
                    .on_connection_handler_event(peer_id, connection_id, shrex_sub_ev)
            }
            Either::Right(req_resp_ev) => {
                self.req_resp
                    .on_connection_handler_event(peer_id, connection_id, req_resp_ev)
            }
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        let _ = self.pool_tracker.poll(cx);

        if let Poll::Ready(ev) = self.shrex_sub.poll(cx)
            && let Poll::Ready(ev) = self.handle_shrex_event(ev)
        {
            return Poll::Ready(ev);
        }

        if let Poll::Ready(ev) = self.req_resp.poll(cx) {
            if let ToSwarm::GenerateEvent(ev) = ev {
                println!("{ev:?}");
            } else {
                return Poll::Ready(
                    ev.map_out(|_| unreachable!("GenerateEvent handled"))
                        .map_in(Either::Right),
                );
            }
        }

        Poll::Pending
    }
}

type ConnHandlerSelect = ConnectionHandlerSelect<
    THandler<gossipsub::Behaviour>,
    THandler<request_response::Behaviour<TodoCodec>>,
>;

pub(crate) struct ConnHandler(ConnHandlerSelect);

impl ConnectionHandler for ConnHandler {
    type ToBehaviour = <ConnHandlerSelect as ConnectionHandler>::ToBehaviour;
    type FromBehaviour = <ConnHandlerSelect as ConnectionHandler>::FromBehaviour;
    type InboundProtocol = <ConnHandlerSelect as ConnectionHandler>::InboundProtocol;
    type InboundOpenInfo = <ConnHandlerSelect as ConnectionHandler>::InboundOpenInfo;
    type OutboundProtocol = <ConnHandlerSelect as ConnectionHandler>::OutboundProtocol;
    type OutboundOpenInfo = <ConnHandlerSelect as ConnectionHandler>::OutboundOpenInfo;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        self.0.listen_protocol()
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        self.0.poll(cx)
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        self.0.on_behaviour_event(event)
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<
            '_,
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        self.0.on_connection_event(event)
    }

    fn connection_keep_alive(&self) -> bool {
        self.0.connection_keep_alive()
    }

    fn poll_close(&mut self, cx: &mut Context<'_>) -> Poll<Option<Self::ToBehaviour>> {
        self.0.poll_close(cx)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TodoCodec;

#[async_trait]
impl Codec for TodoCodec {
    type Protocol = StreamProtocol;
    type Request = ();
    type Response = ();

    async fn read_request<T>(
        &mut self,
        _: &Self::Protocol,
        _io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        todo!()
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        _io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        todo!()
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        _io: &mut T,
        _req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        todo!()
    }

    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        _io: &mut T,
        _resps: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        todo!()
    }
}
