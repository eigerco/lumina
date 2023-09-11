use std::io;

use async_trait::async_trait;
use celestia_proto::p2p::pb::{HeaderRequest, HeaderResponse};
use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use libp2p::request_response::{self, Codec, ProtocolSupport};
use libp2p::StreamProtocol;
use prost::length_delimiter_len;
use prost::Message;

use crate::utils::stream_protocol_id;

/// Max request size in bytes
const REQUEST_SIZE_MAXIMUM: u64 = 1024;
/// Max response size in bytes
const RESPONSE_SIZE_MAXIMUM: u64 = 10 * 1024 * 1024;
/// Maximum length of the protobuf length delimiter in bytes
const PROTOBUF_MAX_LENGTH_DELIMITER_LEN: usize = 10;

pub type Behaviour = request_response::Behaviour<HeaderCodec>;
pub type Event = request_response::Event<HeaderRequest, HeaderResponse>;

/// Create a new [`Behaviour`]
pub fn new_behaviour(network: &str) -> Behaviour {
    Behaviour::new(
        [(
            stream_protocol_id(network, "/header-ex/v0.0.3"),
            ProtocolSupport::Full,
        )],
        request_response::Config::default(),
    )
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HeaderCodec;

#[async_trait]
impl Codec for HeaderCodec {
    type Protocol = StreamProtocol;
    type Request = HeaderRequest;
    type Response = HeaderResponse;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut buf = bytes::BytesMut::with_capacity(512);
        buf.resize(PROTOBUF_MAX_LENGTH_DELIMITER_LEN, 0);

        let mut read = 0;
        let len = loop {
            match io.read(&mut buf[read..]).await? {
                0 => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        ReadHeaderError::StreamClosed,
                    ));
                }
                n => read += n,
            };

            if let Ok(len) = prost::decode_length_delimiter(&buf[..read]) {
                break len;
            }
        };

        // const value conversion, safe for 32bit and larger usize
        if len
            > REQUEST_SIZE_MAXIMUM
                .try_into()
                .expect("usize too small to hold REQUEST_SIZE_MAXIMUM value")
        {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                ReadHeaderError::RequestTooLarge(len),
            ));
        }

        let length_delimiter_len = length_delimiter_len(len);

        let prev_buf_len = buf.len();
        buf.resize(length_delimiter_len + len, 0);

        if prev_buf_len < buf.len() {
            // We need to read more
            io.read_exact(&mut buf[read..]).await?;
        }

        Ok(HeaderRequest::decode(&buf[length_delimiter_len..])?)
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

        Ok(HeaderResponse::decode_length_delimited(&vec[..])?)
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
        let data = resp.encode_length_delimited_to_vec();

        io.write_all(data.as_ref()).await?;

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ReadHeaderError {
    #[error("stream closed while trying to get header length")]
    StreamClosed,
    #[error("request too large: {0}")]
    RequestTooLarge(usize),
}
