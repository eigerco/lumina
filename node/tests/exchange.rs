use celestia_node::exchange::HeaderCodec;
use celestia_proto::p2p::pb::{header_request::Data, HeaderRequest};
use futures::io::{AsyncRead, AsyncReadExt, Cursor, Error};
use futures::task::{Context, Poll};
use libp2p::{request_response::Codec, swarm::StreamProtocol};
use prost::Message;
use std::pin::Pin;

#[tokio::test]
async fn test_decode_empty() {
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

    assert_eq!(header_request, decoded_header_request);
}

#[tokio::test]
async fn test_decode_data() {
    let data = b"9999888877776666555544443333222211110000";
    let header_request = HeaderRequest {
        amount: 1,
        data: Some(Data::Hash(data.to_vec())),
    };
    let encoded_header_request = header_request.encode_length_delimited_to_vec();
    let mut reader = Cursor::new(encoded_header_request.clone());

    let stream_protocol = StreamProtocol::new("/foo/bar/v0.1");
    let mut codec = HeaderCodec {};

    let decoded_header_request = codec
        .read_request(&stream_protocol, &mut reader)
        .await
        .unwrap();
    assert_eq!(header_request, decoded_header_request);
}

#[tokio::test]
async fn test_decode_chunked_data() {
    let data = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let header_request = HeaderRequest {
        amount: 1,
        data: Some(Data::Hash(data.to_vec())),
    };
    let encoded_header_request = header_request.encode_length_delimited_to_vec();

    let stream_protocol = StreamProtocol::new("/foo/bar/v0.1");
    let mut codec = HeaderCodec {};
    {
        let mut reader = ChunkyAsyncRead::<_, 1>::new(Cursor::new(encoded_header_request.clone()));
        let decoded_header_request = codec
            .read_request(&stream_protocol, &mut reader)
            .await
            .unwrap();
        assert_eq!(header_request, decoded_header_request);
    }
    {
        let mut reader = ChunkyAsyncRead::<_, 2>::new(Cursor::new(encoded_header_request.clone()));
        let decoded_header_request = codec
            .read_request(&stream_protocol, &mut reader)
            .await
            .unwrap();

        assert_eq!(header_request, decoded_header_request);
    }
    {
        let mut reader = ChunkyAsyncRead::<_, 3>::new(Cursor::new(encoded_header_request.clone()));
        let decoded_header_request = codec
            .read_request(&stream_protocol, &mut reader)
            .await
            .unwrap();

        assert_eq!(header_request, decoded_header_request);
    }
    {
        let mut reader = ChunkyAsyncRead::<_, 4>::new(Cursor::new(encoded_header_request.clone()));
        let decoded_header_request = codec
            .read_request(&stream_protocol, &mut reader)
            .await
            .unwrap();

        assert_eq!(header_request, decoded_header_request);
    }
}

#[tokio::test]
async fn test_chunky_async_read() {
    let read_data = "FOO123";
    let cur0 = Cursor::new(read_data);
    let mut chunky = ChunkyAsyncRead::<_, 3>::new(cur0); // {inner: cur0};

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
                for i in 0..read {
                    buf[i] = small_buf[i];
                }
                Poll::Ready(Ok(read))
            }
            ret @ Poll::Ready(Err(_)) => ret,
            Poll::Pending => Poll::Pending,
        }
    }
}
