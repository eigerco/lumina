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
const REQUEST_SIZE_MAXIMUM: usize = 1024;
/// Max response size in bytes
const RESPONSE_SIZE_MAXIMUM: usize = 10 * 1024 * 1024;
/// Maximum length of the protobuf length delimiter in bytes
const PROTOBUF_MAX_LENGTH_DELIMITER_LEN: usize = 10;

pub type Behaviour = request_response::Behaviour<HeaderCodec>;
pub type Event = request_response::Event<Vec<HeaderRequest>, Vec<HeaderResponse>>;

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

impl HeaderCodec {
    async fn read_raw_messages<T, R>(io: &mut T, max_len: usize) -> io::Result<Vec<R>>
    where
        T: AsyncRead + Unpin + Send,
        R: Message + Default,
    {
        let mut messages = vec![];
        let mut buf = Vec::with_capacity(512);
        buf.resize(PROTOBUF_MAX_LENGTH_DELIMITER_LEN, 0);
        'messages: loop {
            let mut read = 0;
            let len = loop {
                match io.read(&mut buf[read..]).await? {
                    0 => {
                        // check if we're between Messages, in which case it's ok to stop
                        if read == 0 {
                            break 'messages;
                        } else {
                            return Err(io::Error::new(
                                io::ErrorKind::UnexpectedEof,
                                ReadHeaderError::StreamClosed,
                            ));
                        }
                    }
                    n => read += n,
                };

                if let Ok(len) = prost::decode_length_delimiter(&buf[..read]) {
                    break len;
                }
            };

            if len > max_len {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    ReadHeaderError::ResponseTooLarge(len),
                ));
            }

            let length_delimiter_len = length_delimiter_len(len);

            let prev_buf_len = buf.len();
            buf.resize(length_delimiter_len + len, 0);

            if prev_buf_len < buf.len() {
                // We need to read more
                io.read_exact(&mut buf[read..]).await?;
            }
            messages.push(R::decode(&buf[length_delimiter_len..])?);
        }

        Ok(messages)
    }
}

#[async_trait]
impl Codec for HeaderCodec {
    type Protocol = StreamProtocol;
    type Request = Vec<HeaderRequest>;
    type Response = Vec<HeaderResponse>;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        HeaderCodec::read_raw_messages(io, REQUEST_SIZE_MAXIMUM).await
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        HeaderCodec::read_raw_messages(io, RESPONSE_SIZE_MAXIMUM).await
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        reqs: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        for req in reqs {
            let data = req.encode_length_delimited_to_vec();

            io.write_all(data.as_ref()).await?;
        }
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

            io.write_all(data.as_ref()).await?;
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ReadHeaderError {
    #[error("stream closed while trying to get header length")]
    StreamClosed,
    #[error("request too large: {0}")]
    ResponseTooLarge(usize),
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
        let mut reader = Cursor::new(encoded_header_request.clone());

        let stream_protocol = StreamProtocol::new("/foo/bar/v0.1");
        let mut codec = HeaderCodec {};

        let decoded_header_request = codec
            .read_request(&stream_protocol, &mut reader)
            .await
            .unwrap();

        assert_eq!(header_request, decoded_header_request[0]);
    }

    #[tokio::test]
    async fn test_decode_header_response_empty() {
        let header_response = HeaderResponse {
            body: vec![],
            status_code: 0,
        };

        let encoded_header_response = header_response.encode_length_delimited_to_vec();
        let mut reader = Cursor::new(encoded_header_response);

        let stream_protocol = StreamProtocol::new("/foo/bar/v0.1");
        let mut codec = HeaderCodec {};

        let decoded_header_response = codec
            .read_response(&stream_protocol, &mut reader)
            .await
            .unwrap();

        assert_eq!(header_response, decoded_header_response[0]);
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
        let mut length_delimiter_buffer = bytes::BytesMut::new();
        encode_length_delimiter(too_long_message_len, &mut length_delimiter_buffer).unwrap();
        dbg!(&length_delimiter_buffer);
        let mut reader = Cursor::new(length_delimiter_buffer);

        let stream_protocol = StreamProtocol::new("/foo/bar/v0.1");
        let mut codec = HeaderCodec {};

        let decoding_error = codec
            .read_response(&stream_protocol, &mut reader)
            .await
            .expect_err("expected error for too large request");
        dbg!(&decoding_error);

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
    async fn test_decode_header_double_request_data() {
        let mut header_request_buffer = bytes::BytesMut::with_capacity(512);
        let header_request0 = HeaderRequest {
            amount: 1,
            data: Some(Data::Hash(
                b"9999888877776666555544443333222211110000".to_vec(),
            )),
        };
        let header_request1 = HeaderRequest {
            amount: 2,
            data: Some(Data::Hash(
                b"0000111122223333444455556666777788889999".to_vec(),
            )),
        };
        header_request0
            .encode_length_delimited(&mut header_request_buffer)
            .unwrap();
        header_request1
            .encode_length_delimited(&mut header_request_buffer)
            .unwrap();
        let mut reader = Cursor::new(header_request_buffer);

        let stream_protocol = StreamProtocol::new("/foo/bar/v0.1");
        let mut codec = HeaderCodec {};

        let decoded_header_request = codec
            .read_request(&stream_protocol, &mut reader)
            .await
            .unwrap();
        assert_eq!(header_request0, decoded_header_request[0]);
        assert_eq!(header_request1, decoded_header_request[1]);
    }

    #[tokio::test]
    async fn test_decode_header_double_response_data() {
        let mut header_response_buffer = bytes::BytesMut::with_capacity(512);
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
            assert_eq!(header_request, decoded_header_request[0]);
        }
        {
            let mut reader =
                ChunkyAsyncRead::<_, 2>::new(Cursor::new(encoded_header_request.clone()));
            let decoded_header_request = codec
                .read_request(&stream_protocol, &mut reader)
                .await
                .unwrap();

            assert_eq!(header_request, decoded_header_request[0]);
        }
        {
            let mut reader =
                ChunkyAsyncRead::<_, 3>::new(Cursor::new(encoded_header_request.clone()));
            let decoded_header_request = codec
                .read_request(&stream_protocol, &mut reader)
                .await
                .unwrap();

            assert_eq!(header_request, decoded_header_request[0]);
        }
        {
            let mut reader =
                ChunkyAsyncRead::<_, 4>::new(Cursor::new(encoded_header_request.clone()));
            let decoded_header_request = codec
                .read_request(&stream_protocol, &mut reader)
                .await
                .unwrap();

            assert_eq!(header_request, decoded_header_request[0]);
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

        let mut output_buffer: bytes::BytesMut = b"BAR987".as_ref().into();

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
            let mut small_buf = [0u8; CHUNK_SIZE];
            match Pin::new(&mut self.inner).poll_read(cx, &mut small_buf) {
                Poll::Ready(Ok(read)) => {
                    buf[..read].copy_from_slice(&small_buf[..read]);
                    Poll::Ready(Ok(read))
                }
                ret @ Poll::Ready(Err(_)) => ret,
                Poll::Pending => Poll::Pending,
            }
        }
    }
}
