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
pub type Event = request_response::Event<HeaderRequest, Vec<HeaderResponse>>;

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
    async fn read_raw_message<T, R>(
        io: &mut T,
        buf: &mut Vec<u8>,
        max_len: usize,
    ) -> io::Result<Option<R>>
    where
        T: AsyncRead + Unpin + Send,
        R: Message + Default,
    {
        let mut read = buf.len(); // buf might have data from previous iterations

        if buf.len() < 512 {
            // resize to increase the chance of reading all the data in one go
            buf.resize(512, 0)
        }

        let data_len = loop {
            if let Ok(len) = prost::decode_length_delimiter(&buf[..read]) {
                break len;
            }

            if read >= PROTOBUF_MAX_LENGTH_DELIMITER_LEN {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    ReadHeaderError::VarintOverflow,
                ));
            }

            buf.resize(PROTOBUF_MAX_LENGTH_DELIMITER_LEN, 0);

            match io.read(&mut buf[read..]).await? {
                0 => {
                    // check if we're between Messages, in which case it's ok to stop
                    if read == 0 {
                        return Ok(None);
                    } else {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            ReadHeaderError::StreamClosed,
                        ));
                    }
                }
                n => read += n,
            };

            // truncate buffer to the data that was actually read
            buf.resize(read, 0);
        };
        let length_delimiter_len = length_delimiter_len(data_len);
        let single_message_length = length_delimiter_len + data_len;

        if data_len > max_len {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                ReadHeaderError::ResponseTooLarge(data_len),
            ));
        }

        if read < single_message_length {
            // we need to read more
            buf.resize(single_message_length, 0);
            io.read_exact(&mut buf[read..single_message_length]).await?;
        }

        let val = R::decode(&buf[length_delimiter_len..single_message_length])?;

        // we've read past one message when trying to get length delimiter, need to handle
        // partially read data in the buffer
        if single_message_length < read {
            buf.drain(..single_message_length);
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
        let mut buf = Vec::with_capacity(512);

        HeaderCodec::read_raw_message(io, &mut buf, REQUEST_SIZE_MAXIMUM)
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
        let mut buf = Vec::with_capacity(512);
        loop {
            match HeaderCodec::read_raw_message(io, &mut buf, RESPONSE_SIZE_MAXIMUM).await {
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

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ReadHeaderError {
    #[error("stream closed while trying to get header length")]
    StreamClosed,
    #[error("varint overflow")]
    VarintOverflow,
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
        //let mut reader = Cursor::new(encoded_header_response);

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
        let mut length_delimiter_buffer = bytes::BytesMut::new();
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
            HeaderCodec::read_raw_message::<_, HeaderRequest>(&mut reader, &mut buf, 512)
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
