use std::fmt::Display;
use std::sync::{Arc, LazyLock};
use std::task::{Context, Poll};

use celestia_types::eds::ExtendedDataSquare;
use celestia_types::namespace_data::NamespaceData;
use celestia_types::nmt::Namespace;
use celestia_types::row::Row;
use celestia_types::sample::Sample;
use celestia_types::{DataAvailabilityHeader, Hash};
use libp2p::core::Endpoint;
use libp2p::core::transport::PortUse;
use libp2p::gossipsub::{self, Message};
use libp2p::identity::Keypair;
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandlerInEvent, THandlerOutEvent,
    ToSwarm,
};
use libp2p::{Multiaddr, PeerId};
use thiserror::Error;
use tokio::sync::oneshot;
use tracing::debug;

mod client;
mod codec;
mod pool_tracker;

use crate::p2p::P2pError;
use crate::p2p::shrex::client::Client;
use crate::p2p::shrex::pool_tracker::{EdsNotification, PoolTracker};
use crate::peer_tracker::PeerTracker;
use crate::store::Store;

const ROW_PROTOCOL_ID: &str = "/shrex/v0.1.0/row_v0";
const SAMPLE_PROTOCOL_ID: &str = "/shrex/v0.1.0/sample_v0";
const NAMESPACE_DATA_PROTOCOL_ID: &str = "/shrex/v0.1.0/nd_v0";
const EDS_PROTOCOL_ID: &str = "/shrex/v0.1.0/eds_v0";

static EMPTY_EDS: LazyLock<ExtendedDataSquare> = LazyLock::new(ExtendedDataSquare::empty);
static EMPTY_EDS_DAH: LazyLock<DataAvailabilityHeader> =
    LazyLock::new(|| DataAvailabilityHeader::from_eds(&EMPTY_EDS));
static EMPTY_EDS_DATA_HASH: LazyLock<Hash> = LazyLock::new(|| EMPTY_EDS_DAH.hash());

pub(crate) type Result<T, E = ShrExError> = std::result::Result<T, E>;

pub(crate) struct Config<'a, S> {
    pub network_id: &'a str,
    pub local_keypair: &'a Keypair,
    pub header_store: Arc<S>,
    pub stream_ctrl: libp2p_stream::Control,
}

pub(crate) struct Behaviour<S>
where
    S: Store + 'static,
{
    inner: InnerBehaviour,
    client: Client<S>,
    pool_tracker: PoolTracker<S>,
}

#[derive(NetworkBehaviour)]
pub(crate) struct InnerBehaviour {
    // TODO: this is a workaround, should be replaced with a real floodsub
    // rust-libp2p implementation of the floodsub isn't compliant with a spec,
    // so we work that around by using a gossipsub configured with floodsub support.
    // Gossipsub will always receive from and forward messages to all the floodsub peers.
    // Since we always maintain a connection with a few bridge (and possibly archival)
    // nodes, then if we assume those nodes correctly use only floodsub protocol,
    // we cannot be isolated in a way described in a shrex-sub spec:
    // https://github.com/celestiaorg/celestia-node/blob/76db37cc4ac09e892122a081b8bea24f87899f11/specs/src/shrex/shrex-sub.md#why-not-gossipsub
    shrex_sub: gossipsub::Behaviour,
}

#[derive(Debug)]
pub(crate) enum Event {
    SchedulePendingRequests,

    PoolUpdate {
        add_peers: Vec<PeerId>,
        blacklist_peers: Vec<PeerId>,
    },
}

#[derive(Debug, Error)]
pub enum ShrExError {
    /// Request cancelled because [`Node`] is stopping.
    ///
    /// [`Node`]: crate::node::Node
    #[error("Request cancelled because `Node` is stopping")]
    RequestCancelled,

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Max tries reached")]
    MaxTriesReached,
}

impl ShrExError {
    pub(crate) fn invalid_request(s: impl Display) -> ShrExError {
        ShrExError::InvalidRequest(s.to_string())
    }
}

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

        let client = Client::new(&config);
        let pool_tracker = PoolTracker::new(config.header_store);

        Ok(Self {
            inner: InnerBehaviour { shrex_sub },
            client,
            pool_tracker,
        })
    }

    fn on_to_swarm(
        &mut self,
        ev: ToSwarm<InnerBehaviourEvent, THandlerInEvent<InnerBehaviour>>,
    ) -> Option<ToSwarm<Event, THandlerInEvent<Self>>> {
        match ev {
            ToSwarm::GenerateEvent(InnerBehaviourEvent::ShrexSub(ev)) => {
                self.on_shrex_sub_event(ev);
                None
            }
            _ => Some(ev.map_out(|_| unreachable!("GenerateEvent handled"))),
        }
    }

    fn on_shrex_sub_event(&mut self, ev: gossipsub::Event) {
        match ev {
            gossipsub::Event::Message {
                message: Message { source, data, .. },
                message_id,
                propagation_source,
                ..
            } => {
                let acceptance = if let Ok(EdsNotification { height, data_hash }) =
                    EdsNotification::deserialize_and_validate(data.as_ref())
                    && let Some(peer_id) = source
                {
                    if height == 0 || data_hash == *EMPTY_EDS_DATA_HASH {
                        // hardly reject messages with invalid height or about empty blocks
                        gossipsub::MessageAcceptance::Reject
                    } else {
                        self.pool_tracker
                            .add_peer_for_hash(peer_id, data_hash, height);
                        // return `ignore` for all other messages, so we do not rebroadcast them
                        gossipsub::MessageAcceptance::Ignore
                    }
                } else {
                    // if message was improperly encoded or didn't have the peer id that
                    // advertises data availability, we reject it too
                    debug!("Invalid shrex sub message");
                    gossipsub::MessageAcceptance::Reject
                };

                self.inner.shrex_sub.report_message_validation_result(
                    &message_id,
                    &propagation_source,
                    acceptance,
                );
            }

            gossipsub::Event::SlowPeer {
                peer_id,
                failed_messages,
            } => debug!("shrex-sub {peer_id} slow: {failed_messages:?}"),

            _ => {}
        }
    }

    pub(crate) fn stop(&mut self) {
        self.client.on_stop();
    }

    pub(crate) fn schedule_pending_requests(&mut self, peer_tracker: &PeerTracker) {
        self.client
            .schedule_pending_requests(peer_tracker, &self.pool_tracker);
    }

    pub(crate) async fn get_row(
        &mut self,
        height: u64,
        index: u16,
        respond_to: oneshot::Sender<Result<Row, P2pError>>,
    ) {
        self.client.get_row(height, index, respond_to).await;
    }

    pub(crate) async fn get_sample(
        &mut self,
        height: u64,
        row_index: u16,
        column_index: u16,
        respond_to: oneshot::Sender<Result<Sample, P2pError>>,
    ) {
        self.client
            .get_sample(height, row_index, column_index, respond_to)
            .await;
    }

    pub(crate) async fn get_namespace_data(
        &mut self,
        height: u64,
        namespace: Namespace,
        respond_to: oneshot::Sender<Result<NamespaceData, P2pError>>,
    ) {
        self.client
            .get_namespace_data(height, namespace, respond_to)
            .await;
    }

    pub(crate) async fn get_eds(
        &mut self,
        height: u64,
        respond_to: oneshot::Sender<Result<ExtendedDataSquare, P2pError>>,
    ) {
        self.client.get_eds(height, respond_to).await
    }
}

impl<S> NetworkBehaviour for Behaviour<S>
where
    S: Store + 'static,
{
    type ConnectionHandler = <InnerBehaviour as NetworkBehaviour>::ConnectionHandler;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<Self::ConnectionHandler, ConnectionDenied> {
        self.inner.handle_established_inbound_connection(
            connection_id,
            peer,
            local_addr,
            remote_addr,
        )
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
        port_use: PortUse,
    ) -> Result<Self::ConnectionHandler, ConnectionDenied> {
        self.inner.handle_established_outbound_connection(
            connection_id,
            peer,
            addr,
            role_override,
            port_use,
        )
    }

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.inner
            .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        self.inner.handle_pending_outbound_connection(
            connection_id,
            maybe_peer,
            addresses,
            effective_role,
        )
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        self.inner.on_swarm_event(event)
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        self.inner
            .on_connection_handler_event(peer_id, connection_id, event)
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Poll::Ready(ev) = self.inner.poll(cx)
            && let Some(ev) = self.on_to_swarm(ev)
        {
            return Poll::Ready(ev);
        }

        if let Poll::Ready(Some(ev)) = self.pool_tracker.poll(cx) {
            return Poll::Ready(ToSwarm::GenerateEvent(ev));
        }

        if let Poll::Ready(ev) = self.client.poll(cx) {
            return Poll::Ready(ToSwarm::GenerateEvent(ev));
        }

        Poll::Pending
    }
}
