use std::ops::{Deref, DerefMut};
use std::task::{Context, Poll};

use libp2p::{
    core::Endpoint,
    kad,
    multiaddr::{Multiaddr, Protocol},
    swarm::{
        ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
        THandlerOutEvent, ToSwarm,
    },
    PeerId,
};

type Kad = kad::Behaviour<kad::store::MemoryStore>;

pub(crate) struct Behaviour(Kad);

impl Behaviour {
    pub(crate) fn new(inner: Kad) -> Behaviour {
        Behaviour(inner)
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = THandler<Kad>;
    type ToSwarm = kad::Event;

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        // The that `libp2p::kad::Behaviour` uses to flow of dialing to a peer is as follows:
        //
        // 1. Kad's Behaviour discovers a new peer and its addresses.
        // 2. Kad's Behaviour dials with `DialOpts::peer_id()` without setting the addresses.
        // 3. `Swarm` calls `Behaviour::handle_pending_outbound_connection` to resolve the addresses.
        // 4. Kad's Behaviour returns a vector of the discovered addresses.
        // 5. The addresses returned from the above step are NOT passed to any other Behaviour's.
        //
        // Here we intercept the return of Kad's Behaviour and cononicalize all `/tls/ws`
        // addresses to `/wss` addresses.
        let mut new_addrs = self.0.handle_pending_outbound_connection(
            connection_id,
            maybe_peer,
            addresses,
            effective_role,
        )?;

        for addr in new_addrs.iter_mut() {
            if let Some(new_addr) = tls_ws_to_wss(addr) {
                *addr = new_addr;
            }
        }

        for addr in addresses {
            if let Some(new_addr) = tls_ws_to_wss(addr) {
                new_addrs.push(new_addr);
            }
        }

        Ok(new_addrs)
    }

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.0
            .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)
    }

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.0
            .handle_established_inbound_connection(connection_id, peer, local_addr, remote_addr)
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.0
            .handle_established_outbound_connection(connection_id, peer, addr, role_override)
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        self.0
            .on_connection_handler_event(peer_id, connection_id, event)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        self.0.on_swarm_event(event)
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        self.0.poll(cx)
    }
}

impl Deref for Behaviour {
    type Target = Kad;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Behaviour {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

fn tls_ws_to_wss(addr: &Multiaddr) -> Option<Multiaddr> {
    let tls_idx = addr.iter().position(|proto| proto == Protocol::Tls)?;

    let Some(Protocol::Ws(s)) = addr.iter().nth(tls_idx + 1) else {
        return None;
    };

    let addr = addr
        .iter()
        .enumerate()
        .fold(Multiaddr::empty(), |mut addr, (i, proto)| {
            if i == tls_idx {
                // Skip Protocol::Tls
            } else if i == tls_idx + 1 {
                // Replace Protocol::Ws with Protocol::Wss
                addr.push(Protocol::Wss(s.clone()));
            } else {
                addr.push(proto);
            }

            addr
        });

    Some(addr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::async_test;

    #[async_test]
    async fn tls_ws() {
        let addr = "/dns4/dev.lumina.eiger.co/tcp/2121/tls/ws/p2p/12D3KooWJF4tkwBrycYhriE4nuYAo3Y8DESQzdN2tPWwndWe4KUd".parse().unwrap();
        let expected_addr = "/dns4/dev.lumina.eiger.co/tcp/2121/wss/p2p/12D3KooWJF4tkwBrycYhriE4nuYAo3Y8DESQzdN2tPWwndWe4KUd".parse().unwrap();

        let new_addr = tls_ws_to_wss(&addr).unwrap();
        assert_eq!(new_addr, expected_addr);
    }

    #[async_test]
    async fn non_translatable() {
        let addrs = [
            "/dns4/dev.lumina.eiger.co/tcp/2121/tls/p2p/12D3KooWJF4tkwBrycYhriE4nuYAo3Y8DESQzdN2tPWwndWe4KUd",
            "/dns4/dev.lumina.eiger.co/tcp/2121/wss/p2p/12D3KooWJF4tkwBrycYhriE4nuYAo3Y8DESQzdN2tPWwndWe4KUd",
            "/dns4/dev.lumina.eiger.co/tcp/2121/p2p/12D3KooWJF4tkwBrycYhriE4nuYAo3Y8DESQzdN2tPWwndWe4KUd",
        ];

        for addr in addrs {
            let addr = addr.parse().unwrap();
            assert!(tls_ws_to_wss(&addr).is_none());
        }
    }
}
