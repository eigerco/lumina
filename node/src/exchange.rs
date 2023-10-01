use std::io;
use std::sync::Arc;
use std::task::{Context, Poll};

use async_trait::async_trait;
use celestia_proto::p2p::pb::{HeaderRequest, HeaderResponse};
use celestia_types::ExtendedHeader;
use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use libp2p::{
    core::Endpoint,
    request_response::{self, Codec, InboundFailure, OutboundFailure, ProtocolSupport},
    swarm::{
        ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, PollParameters,
        THandlerInEvent, THandlerOutEvent, ToSwarm,
    },
    Multiaddr, PeerId, StreamProtocol,
};
use prost::{length_delimiter_len, Message};
use tracing::instrument;

mod client;
mod server;
mod utils;

use crate::exchange::client::ExchangeClientHandler;
use crate::exchange::server::ExchangeServerHandler;
use crate::p2p::P2pError;
use crate::peer_tracker::PeerTracker;
use crate::store::Store;
use crate::utils::{protocol_id, OneshotResultSender};

/// Max request size in bytes
const REQUEST_SIZE_MAXIMUM: usize = 1024;
/// Max response size in bytes
const RESPONSE_SIZE_MAXIMUM: usize = 10 * 1024 * 1024;
/// Maximum length of the protobuf length delimiter in bytes
const PROTOBUF_MAX_LENGTH_DELIMITER_LEN: usize = 10;

type RequestType = HeaderRequest;
type ResponseType = Vec<HeaderResponse>;
type ReqRespBehaviour = request_response::Behaviour<HeaderCodec>;
type ReqRespEvent = request_response::Event<RequestType, ResponseType>;
type ReqRespMessage = request_response::Message<RequestType, ResponseType>;

pub(crate) struct ExchangeBehaviour<S>
where
    S: Store + 'static,
{
    req_resp: ReqRespBehaviour,
    client_handler: ExchangeClientHandler,
    server_handler: ExchangeServerHandler<S>,
}

pub(crate) struct ExchangeConfig<'a, S> {
    pub network_id: &'a str,
    pub peer_tracker: Arc<PeerTracker>,
    pub header_store: Arc<S>,
}

#[derive(Debug, thiserror::Error)]
pub enum ExchangeError {
    #[error("Header not found")]
    HeaderNotFound,

    #[error("Invalid response")]
    InvalidResponse,

    #[error("Invalid request")]
    InvalidRequest,

    #[error("Inbound failure: {0}")]
    InboundFailure(InboundFailure),

    #[error("Outbound failure: {0}")]
    OutboundFailure(OutboundFailure),
}

impl<S> ExchangeBehaviour<S>
where
    S: Store + 'static,
{
    pub(crate) fn new(config: ExchangeConfig<'_, S>) -> Self {
        ExchangeBehaviour {
            req_resp: ReqRespBehaviour::new(
                [(
                    protocol_id(config.network_id, "/header-ex/v0.0.3"),
                    ProtocolSupport::Full,
                )],
                request_response::Config::default(),
            ),
            client_handler: ExchangeClientHandler::new(config.peer_tracker),
            server_handler: ExchangeServerHandler::new(config.header_store),
        }
    }

    #[instrument(level = "trace", skip(self, respond_to))]
    pub(crate) fn send_request(
        &mut self,
        request: HeaderRequest,
        respond_to: OneshotResultSender<Vec<ExtendedHeader>, P2pError>,
    ) {
        self.client_handler
            .on_send_request(&mut self.req_resp, request, respond_to);
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

    #[instrument(level = "trace", skip_all)]
    fn on_req_resp_event(&mut self, ev: ReqRespEvent) {
        match ev {
            // Received a response for an ongoing outbound request
            ReqRespEvent::Message {
                message:
                    ReqRespMessage::Response {
                        request_id,
                        response,
                    },
                peer,
            } => {
                self.client_handler
                    .on_response_received(peer, request_id, response);
            }

            // Failure while client requests
            ReqRespEvent::OutboundFailure {
                peer,
                request_id,
                error,
            } => {
                self.client_handler.on_failure(peer, request_id, error);
            }

            // Received new inbound request
            ReqRespEvent::Message {
                message:
                    ReqRespMessage::Request {
                        request_id,
                        request,
                        channel,
                    },
                peer,
            } => {
                self.server_handler
                    .on_request_received(peer, request_id, request, channel);
            }

            // Response to inbound request was sent
            ReqRespEvent::ResponseSent { peer, request_id } => {
                self.server_handler.on_response_sent(peer, request_id);
            }

            // Failure while server responds
            ReqRespEvent::InboundFailure {
                peer,
                request_id,
                error,
            } => {
                self.server_handler.on_failure(peer, request_id, error);
            }
        }
    }
}

impl<S> NetworkBehaviour for ExchangeBehaviour<S>
where
    S: Store + 'static,
{
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

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.req_resp
            .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        self.req_resp.handle_pending_outbound_connection(
            connection_id,
            maybe_peer,
            addresses,
            effective_role,
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
        loop {
            if let Poll::Ready(ev) = self.req_resp.poll(cx, params) {
                if let Some(ev) = self.on_to_swarm(ev) {
                    return Poll::Ready(ev);
                }

                continue;
            }
            if self.server_handler.poll(cx, &mut self.req_resp).is_ready() {
                continue;
            }

            return Poll::Pending;
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct HeaderCodec;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ReadHeaderError {
    #[error("stream closed while trying to get header length")]
    StreamClosed,
    #[error("varint overflow")]
    VarintOverflow,
    #[error("request too large: {0}")]
    ResponseTooLarge(usize),
}

impl HeaderCodec {
    async fn read_message<R, T>(
        reader: &mut R,
        buf: &mut Vec<u8>,
        max_len: usize,
    ) -> io::Result<Option<T>>
    where
        R: AsyncRead + Unpin + Send,
        T: Message + Default,
    {
        let mut read_len = buf.len(); // buf might have data from previous iterations

        if read_len < 512 {
            // resize to increase the chance of reading all the data in one go
            buf.resize(512, 0)
        }

        let data_len = loop {
            if let Ok(len) = prost::decode_length_delimiter(&buf[..read_len]) {
                break len;
            }

            if read_len >= PROTOBUF_MAX_LENGTH_DELIMITER_LEN {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    ReadHeaderError::VarintOverflow,
                ));
            }

            match reader.read(&mut buf[read_len..]).await? {
                0 => {
                    // check if we're between Messages, in which case it's ok to stop
                    if read_len == 0 {
                        return Ok(None);
                    } else {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            ReadHeaderError::StreamClosed,
                        ));
                    }
                }
                n => read_len += n,
            };
        };

        // truncate buffer to the data that was actually read_len
        buf.truncate(read_len);

        let length_delimiter_len = length_delimiter_len(data_len);
        let single_message_len = length_delimiter_len + data_len;

        if data_len > max_len {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                ReadHeaderError::ResponseTooLarge(data_len),
            ));
        }

        if read_len < single_message_len {
            // we need to read_len more
            buf.resize(single_message_len, 0);
            reader
                .read_exact(&mut buf[read_len..single_message_len])
                .await?;
        }

        let val = T::decode(&buf[length_delimiter_len..single_message_len])?;

        // we've read_len past one message when trying to get length delimiter, need to handle
        // partially read_len data in the buffer
        if single_message_len < read_len {
            buf.drain(..single_message_len);
        } else {
            buf.clear();
        }

        Ok(Some(val))
    }
}

#[async_trait]
impl Codec for HeaderCodec {
    type Protocol = StreamProtocol;
    type Request = HeaderRequest;
    type Response = Vec<HeaderResponse>;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut buf = Vec::new();

        HeaderCodec::read_message(io, &mut buf, REQUEST_SIZE_MAXIMUM)
            .await?
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::UnexpectedEof, ReadHeaderError::StreamClosed)
            })
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut messages = vec![];
        let mut buf = Vec::new();
        loop {
            match HeaderCodec::read_message(io, &mut buf, RESPONSE_SIZE_MAXIMUM).await {
                Ok(None) => break,
                Ok(Some(msg)) => messages.push(msg),
                Err(e) => {
                    return Err(e);
                }
            };
        }
        Ok(messages)
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
        resps: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        for resp in resps {
            let data = resp.encode_length_delimited_to_vec();

            io.write_all(&data).await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{HeaderCodec, ReadHeaderError, REQUEST_SIZE_MAXIMUM, RESPONSE_SIZE_MAXIMUM};
    use bytes::BytesMut;
    use celestia_proto::p2p::pb::{header_request::Data, HeaderRequest, HeaderResponse};
    use futures::io::{AsyncRead, AsyncReadExt, Cursor, Error};
    use futures::task::{Context, Poll};
    use libp2p::{request_response::Codec, swarm::StreamProtocol};
    use prost::{encode_length_delimiter, Message};
    use std::io::ErrorKind;
    use std::pin::Pin;

    #[tokio::test]
    async fn test_decode_header_request_empty() {
        let header_request = HeaderRequest {
            amount: 0,
            data: None,
        };

        let encoded_header_request = header_request.encode_length_delimited_to_vec();

        let mut reader = Cursor::new(encoded_header_request);

        let stream_protocol = StreamProtocol::new("/foo/bar/v0.1");
        let mut codec = HeaderCodec {};

        let decoded_header_request = codec
            .read_request(&stream_protocol, &mut reader)
            .await
            .unwrap();

        assert_eq!(header_request, decoded_header_request);
    }

    #[tokio::test]
    async fn test_decode_multiple_small_header_response() {
        const MSG_COUNT: usize = 10;
        let header_response = HeaderResponse {
            body: vec![1, 2, 3],
            status_code: 1,
        };

        let encoded_header_response = header_response.encode_length_delimited_to_vec();

        let mut multi_msg = vec![];
        for _ in 0..MSG_COUNT {
            multi_msg.extend_from_slice(&encoded_header_response);
        }
        let mut reader = Cursor::new(multi_msg);

        let stream_protocol = StreamProtocol::new("/foo/bar/v0.1");
        let mut codec = HeaderCodec {};

        let decoded_header_response = codec
            .read_response(&stream_protocol, &mut reader)
            .await
            .unwrap();

        for decoded_header in decoded_header_response.iter() {
            assert_eq!(&header_response, decoded_header);
        }
        assert_eq!(decoded_header_response.len(), MSG_COUNT);
    }

    #[tokio::test]
    async fn test_decode_header_request_too_large() {
        let too_long_message_len = REQUEST_SIZE_MAXIMUM + 1;
        let mut length_delimiter_buffer = BytesMut::new();
        prost::encode_length_delimiter(REQUEST_SIZE_MAXIMUM + 1, &mut length_delimiter_buffer)
            .unwrap();
        let mut reader = Cursor::new(length_delimiter_buffer);

        let stream_protocol = StreamProtocol::new("/foo/bar/v0.1");
        let mut codec = HeaderCodec {};

        let decoding_error = codec
            .read_request(&stream_protocol, &mut reader)
            .await
            .expect_err("expected error for too large request");

        assert_eq!(decoding_error.kind(), ErrorKind::Other);
        let inner_err = decoding_error
            .get_ref()
            .unwrap()
            .downcast_ref::<ReadHeaderError>()
            .unwrap();
        assert_eq!(
            inner_err,
            &ReadHeaderError::ResponseTooLarge(too_long_message_len)
        );
    }

    #[tokio::test]
    async fn test_decode_header_response_too_large() {
        let too_long_message_len = RESPONSE_SIZE_MAXIMUM + 1;
        let mut length_delimiter_buffer = BytesMut::new();
        encode_length_delimiter(too_long_message_len, &mut length_delimiter_buffer).unwrap();
        let mut reader = Cursor::new(length_delimiter_buffer);

        let stream_protocol = StreamProtocol::new("/foo/bar/v0.1");
        let mut codec = HeaderCodec {};

        let decoding_error = codec
            .read_response(&stream_protocol, &mut reader)
            .await
            .expect_err("expected error for too large request");

        assert_eq!(decoding_error.kind(), ErrorKind::Other);
        let inner_err = decoding_error
            .get_ref()
            .unwrap()
            .downcast_ref::<ReadHeaderError>()
            .unwrap();
        assert_eq!(
            inner_err,
            &ReadHeaderError::ResponseTooLarge(too_long_message_len)
        );
    }

    #[tokio::test]
    async fn test_invalid_varint() {
        // 10 consecutive bytes with continuation bit set + 1 byte, which is longer than allowed
        //    for length delimiter
        let varint = [
            0b1000_0000,
            0b1000_0000,
            0b1000_0000,
            0b1000_0000,
            0b1000_0000,
            0b1000_0000,
            0b1000_0000,
            0b1000_0000,
            0b1000_0000,
            0b1000_0000,
            0b0000_0001,
        ];
        let mut reader = Cursor::new(varint);

        let mut buf = vec![];
        let decoding_error =
            HeaderCodec::read_message::<_, HeaderRequest>(&mut reader, &mut buf, 512)
                .await
                .expect_err("expected varint overflow");

        assert_eq!(decoding_error.kind(), ErrorKind::InvalidData);
        let inner_err = decoding_error
            .get_ref()
            .unwrap()
            .downcast_ref::<ReadHeaderError>()
            .unwrap();
        assert_eq!(inner_err, &ReadHeaderError::VarintOverflow);
    }

    #[tokio::test]
    async fn test_decode_header_double_response_data() {
        let mut header_response_buffer = BytesMut::with_capacity(512);
        let header_response0 = HeaderResponse {
            body: b"9999888877776666555544443333222211110000".to_vec(),
            status_code: 1,
        };
        let header_response1 = HeaderResponse {
            body: b"0000111122223333444455556666777788889999".to_vec(),
            status_code: 2,
        };
        header_response0
            .encode_length_delimited(&mut header_response_buffer)
            .unwrap();
        header_response1
            .encode_length_delimited(&mut header_response_buffer)
            .unwrap();
        let mut reader = Cursor::new(header_response_buffer);

        let stream_protocol = StreamProtocol::new("/foo/bar/v0.1");
        let mut codec = HeaderCodec {};

        let decoded_header_response = codec
            .read_response(&stream_protocol, &mut reader)
            .await
            .unwrap();
        assert_eq!(header_response0, decoded_header_response[0]);
        assert_eq!(header_response1, decoded_header_response[1]);
    }

    #[tokio::test]
    async fn test_decode_header_request_chunked_data() {
        let data = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let header_request = HeaderRequest {
            amount: 1,
            data: Some(Data::Hash(data.to_vec())),
        };
        let encoded_header_request = header_request.encode_length_delimited_to_vec();

        let stream_protocol = StreamProtocol::new("/foo/bar/v0.1");
        let mut codec = HeaderCodec {};
        {
            let mut reader =
                ChunkyAsyncRead::<_, 1>::new(Cursor::new(encoded_header_request.clone()));
            let decoded_header_request = codec
                .read_request(&stream_protocol, &mut reader)
                .await
                .unwrap();
            assert_eq!(header_request, decoded_header_request);
        }
        {
            let mut reader =
                ChunkyAsyncRead::<_, 2>::new(Cursor::new(encoded_header_request.clone()));
            let decoded_header_request = codec
                .read_request(&stream_protocol, &mut reader)
                .await
                .unwrap();

            assert_eq!(header_request, decoded_header_request);
        }
        {
            let mut reader =
                ChunkyAsyncRead::<_, 3>::new(Cursor::new(encoded_header_request.clone()));
            let decoded_header_request = codec
                .read_request(&stream_protocol, &mut reader)
                .await
                .unwrap();

            assert_eq!(header_request, decoded_header_request);
        }
        {
            let mut reader =
                ChunkyAsyncRead::<_, 4>::new(Cursor::new(encoded_header_request.clone()));
            let decoded_header_request = codec
                .read_request(&stream_protocol, &mut reader)
                .await
                .unwrap();

            assert_eq!(header_request, decoded_header_request);
        }
    }

    #[tokio::test]
    async fn test_decode_header_response_chunked_data() {
        let data = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let header_response = HeaderResponse {
            body: data.to_vec(),
            status_code: 2,
        };
        let encoded_header_response = header_response.encode_length_delimited_to_vec();

        let stream_protocol = StreamProtocol::new("/foo/bar/v0.1");
        let mut codec = HeaderCodec {};
        {
            let mut reader =
                ChunkyAsyncRead::<_, 1>::new(Cursor::new(encoded_header_response.clone()));
            let decoded_header_response = codec
                .read_response(&stream_protocol, &mut reader)
                .await
                .unwrap();
            assert_eq!(header_response, decoded_header_response[0]);
        }
        {
            let mut reader =
                ChunkyAsyncRead::<_, 2>::new(Cursor::new(encoded_header_response.clone()));
            let decoded_header_response = codec
                .read_response(&stream_protocol, &mut reader)
                .await
                .unwrap();

            assert_eq!(header_response, decoded_header_response[0]);
        }
        {
            let mut reader =
                ChunkyAsyncRead::<_, 3>::new(Cursor::new(encoded_header_response.clone()));
            let decoded_header_response = codec
                .read_response(&stream_protocol, &mut reader)
                .await
                .unwrap();

            assert_eq!(header_response, decoded_header_response[0]);
        }
        {
            let mut reader =
                ChunkyAsyncRead::<_, 4>::new(Cursor::new(encoded_header_response.clone()));
            let decoded_header_response = codec
                .read_response(&stream_protocol, &mut reader)
                .await
                .unwrap();

            assert_eq!(header_response, decoded_header_response[0]);
        }
    }

    #[tokio::test]
    async fn test_chunky_async_read() {
        let read_data = "FOO123";
        let cur0 = Cursor::new(read_data);
        let mut chunky = ChunkyAsyncRead::<_, 3>::new(cur0);

        let mut output_buffer: BytesMut = b"BAR987".as_ref().into();

        let _ = chunky.read(&mut output_buffer[..]).await.unwrap();
        assert_eq!(output_buffer, b"FOO987".as_ref());
    }

    struct ChunkyAsyncRead<T: AsyncRead + Unpin, const CHUNK_SIZE: usize> {
        inner: T,
    }

    impl<T: AsyncRead + Unpin, const CHUNK_SIZE: usize> ChunkyAsyncRead<T, CHUNK_SIZE> {
        fn new(inner: T) -> Self {
            ChunkyAsyncRead { inner }
        }
    }

    impl<T: AsyncRead + Unpin, const CHUNK_SIZE: usize> AsyncRead for ChunkyAsyncRead<T, CHUNK_SIZE> {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<Result<usize, Error>> {
            let len = buf.len().min(CHUNK_SIZE);
            Pin::new(&mut self.inner).poll_read(cx, &mut buf[..len])
        }
    }
}
