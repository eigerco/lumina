use std::collections::VecDeque;
use std::task::{Context, Poll};

use libp2p::swarm::NotifyHandler;
use libp2p::{
    core::{transport::PortUse, upgrade::DeniedUpgrade, Endpoint},
    swarm::{
        handler::ConnectionEvent, ConnectionDenied, ConnectionHandler, ConnectionHandlerEvent,
        ConnectionId, FromSwarm, NetworkBehaviour, SubstreamProtocol, THandler, THandlerInEvent,
        THandlerOutEvent, ToSwarm,
    },
    Multiaddr, PeerId,
};
use void::Void;

// TODO: Wrap ConnectionLimits in it and exclude limits from trusted peers
pub(crate) struct Behaviour {
    stopping: bool,
    events: VecDeque<ToSwarm<Void, FromBehaviour>>,
}

#[derive(Debug, thiserror::Error)]
#[error("Swarm is stopping")]
struct Stopping;

impl Behaviour {
    pub(crate) fn new() -> Behaviour {
        Behaviour {
            stopping: false,
            events: VecDeque::new(),
        }
    }

    pub(crate) fn set_stopping(&mut self, value: bool) {
        self.stopping = value;
    }

    pub(crate) fn set_keep_alive(&mut self, peer_id: &PeerId, conn_id: ConnectionId, value: bool) {
        self.events.push_back(ToSwarm::NotifyHandler {
            peer_id: *peer_id,
            handler: NotifyHandler::One(conn_id),
            event: FromBehaviour::SetKeepAlive(value),
        });
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = ConnHandler;
    type ToSwarm = Void;

    fn handle_pending_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _local_addr: &Multiaddr,
        _remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        if self.stopping {
            return Err(ConnectionDenied::new(Stopping));
        }

        Ok(())
    }

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _local_addr: &Multiaddr,
        _remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        if self.stopping {
            return Err(ConnectionDenied::new(Stopping));
        }

        Ok(ConnHandler { keep_alive: false })
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _maybe_peer: Option<PeerId>,
        _addresses: &[Multiaddr],
        _effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        if self.stopping {
            return Err(ConnectionDenied::new(Stopping));
        }

        Ok(Vec::new())
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _addr: &Multiaddr,
        _role_override: Endpoint,
        _port_use: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        if self.stopping {
            return Err(ConnectionDenied::new(Stopping));
        }

        Ok(ConnHandler { keep_alive: false })
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: ConnectionId,
        _event: THandlerOutEvent<Self>,
    ) {
    }

    fn on_swarm_event(&mut self, _event: FromSwarm) {}

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        Poll::Pending
    }
}

pub(crate) struct ConnHandler {
    keep_alive: bool,
}

#[derive(Debug)]
pub(crate) enum FromBehaviour {
    SetKeepAlive(bool),
}

impl ConnectionHandler for ConnHandler {
    type ToBehaviour = Void;
    type FromBehaviour = FromBehaviour;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = DeniedUpgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            FromBehaviour::SetKeepAlive(value) => {
                self.keep_alive = value;
            }
        }
    }

    fn on_connection_event(
        &mut self,
        _event: ConnectionEvent<Self::InboundProtocol, Self::OutboundProtocol, (), ()>,
    ) {
    }

    fn connection_keep_alive(&self) -> bool {
        self.keep_alive
    }

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
    ) -> Poll<ConnectionHandlerEvent<Self::OutboundProtocol, (), Self::ToBehaviour>> {
        Poll::Pending
    }
}
