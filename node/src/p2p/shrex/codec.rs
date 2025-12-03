// TODO: remove this
#![allow(unused)]

use std::io;

use async_trait::async_trait;
use celestia_types::row::{Row, RowId};
use celestia_types::sample::{Sample, SampleId};
use futures::{AsyncRead, AsyncWrite};
use libp2p::StreamProtocol;
use libp2p::request_response::{self, Codec, ProtocolSupport};

use crate::utils::{parse_protocol_id, protocol_id, read_up_to};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RowCodec;

#[async_trait]
impl Codec for RowCodec {
    type Protocol = StreamProtocol;
    type Request = RowId;
    type Response = Row;

    async fn read_request<T>(
        &mut self,
        protocol: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        todo!();
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
