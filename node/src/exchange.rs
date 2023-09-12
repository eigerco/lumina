use std::io;
use std::sync::Arc;
use std::task::{Context, Poll};

use async_trait::async_trait;
use celestia_proto::p2p::pb::{HeaderRequest, HeaderResponse};
use celestia_types::ExtendedHeader;
use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use libp2p::core::Endpoint;
use libp2p::request_response::{self, Codec, ProtocolSupport};
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, PollParameters, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};
use libp2p::{Multiaddr, PeerId, StreamProtocol};
use prost::Message as _;

use crate::exchange_client::ExchangeClientHandler;
use crate::exchange_server::ExchangeServerHandler;
use crate::p2p::P2pError;
use crate::peer_tracker::PeerTracker;
use crate::utils::{stream_protocol_id, OneshotResultSender, OneshotResultSenderExt};

/// Max request size in bytes
const REQUEST_SIZE_MAXIMUM: u64 = 1024;
/// Max response size in bytes
const RESPONSE_SIZE_MAXIMUM: u64 = 10 * 1024 * 1024;

type ReqRespBehaviour = request_response::Behaviour<HeaderCodec>;
type ReqRespEvent = request_response::Event<HeaderRequest, Vec<HeaderResponse>>;
type ReqRespMessage = request_response::Message<HeaderRequest, Vec<HeaderResponse>>;

pub(crate) struct ExchangeBehaviour {
    req_resp: ReqRespBehaviour,
    peer_tracker: Arc<PeerTracker>,
    client_handler: ExchangeClientHandler,
    server_handler: ExchangeServerHandler,
}

pub(crate) struct ExchangeConfig<'a> {
    pub network_id: &'a str,
    pub peer_tracker: Arc<PeerTracker>,
}

impl ExchangeBehaviour {
    pub(crate) fn new(config: ExchangeConfig<'_>) -> Self {
        ExchangeBehaviour {
            req_resp: ReqRespBehaviour::new(
                [(
                    stream_protocol_id(config.network_id, "/header-ex/v0.0.3"),
                    ProtocolSupport::Full,
                )],
                request_response::Config::default(),
            ),
            peer_tracker: config.peer_tracker,
            client_handler: ExchangeClientHandler::new(),
            server_handler: ExchangeServerHandler::new(),
        }
    }

    #[tracing::instrument(level = "trace", skip(self, respond_to))]
    pub(crate) fn send_request(
        &mut self,
        request: HeaderRequest,
        respond_to: OneshotResultSender<Vec<ExtendedHeader>, P2pError>,
    ) {
        if request.amount == 0 {
            // TODO: is this what celestia-node is doing?
            respond_to.maybe_send_ok(Vec::new());
            return;
        }

        let Some(peer) = self.peer_tracker.get_best() else {
            respond_to.maybe_send_err(P2pError::NoPeers);
            return;
        };

        let amount = request.amount;
        let req_id = self.req_resp.send_request(&peer, request);

        self.client_handler
            .on_request_initiated(req_id, amount, respond_to);
    }

    fn on_to_swarm(
        &mut self,
        ev: ToSwarm<ReqRespEvent, THandlerInEvent<ReqRespBehaviour>>,
    ) -> Option<ToSwarm<(), THandlerInEvent<Self>>> {
        match ev {
            ToSwarm::GenerateEvent(ev) => {
                self.on_req_resp_event(ev);
                None
            }
            _ => Some(ev.map_out(|_| ())),
        }
    }

    fn on_req_resp_event(&mut self, ev: ReqRespEvent) {
        match ev {
            // Received a response for an ongoing outbound request
            ReqRespEvent::Message {
                message:
                    ReqRespMessage::Response {
                        request_id,
                        response,
                    },
                ..
            } => {
                self.client_handler
                    .on_response_received(request_id, response);
            }

            // Failure while client requests
            ReqRespEvent::OutboundFailure {
                request_id, error, ..
            } => {
                self.client_handler.on_failure(request_id, error);
            }

            // Received new inbound request
            ReqRespEvent::Message {
                message:
                    ReqRespMessage::Request {
                        request_id,
                        request,
                        channel,
                    },
                ..
            } => {
                self.server_handler
                    .on_request_received(request_id, request, channel);
            }

            // Response to inbound request was sent
            ReqRespEvent::ResponseSent { request_id, .. } => {
                self.server_handler.on_response_sent(request_id);
            }

            // Failure while server responds
            ReqRespEvent::InboundFailure {
                request_id, error, ..
            } => {
                self.server_handler.on_inbound_failure(request_id, error);
            }
        }
    }
}

impl NetworkBehaviour for ExchangeBehaviour {
    type ConnectionHandler = <ReqRespBehaviour as NetworkBehaviour>::ConnectionHandler;
    type ToSwarm = ();

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<Self::ConnectionHandler, ConnectionDenied> {
        self.req_resp.handle_established_inbound_connection(
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
    ) -> Result<Self::ConnectionHandler, ConnectionDenied> {
        self.req_resp.handle_established_outbound_connection(
            connection_id,
            peer,
            addr,
            role_override,
        )
    }

    fn on_swarm_event(&mut self, event: FromSwarm<'_, Self::ConnectionHandler>) {
        self.req_resp.on_swarm_event(event)
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        self.req_resp
            .on_connection_handler_event(peer_id, connection_id, event)
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        while let Poll::Ready(ev) = self.req_resp.poll(cx, params) {
            if let Some(ev) = self.on_to_swarm(ev) {
                return Poll::Ready(ev);
            }
        }

        Poll::Pending
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct HeaderCodec;

#[async_trait]
impl Codec for HeaderCodec {
    type Protocol = StreamProtocol;
    type Request = HeaderRequest;
    type Response = Vec<HeaderResponse>;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut vec = Vec::new();

        io.take(REQUEST_SIZE_MAXIMUM).read_to_end(&mut vec).await?;

        Ok(HeaderRequest::decode_length_delimited(&vec[..])?)
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut vec = Vec::new();

        io.take(RESPONSE_SIZE_MAXIMUM).read_to_end(&mut vec).await?;

        // TODO
        Ok(vec![HeaderResponse::decode_length_delimited(&vec[..])?])
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let data = req.encode_length_delimited_to_vec();

        io.write_all(data.as_ref()).await?;

        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        resp: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        // TODO
        let data = resp[0].encode_length_delimited_to_vec();

        io.write_all(data.as_ref()).await?;

        Ok(())
    }
}
