use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use celestia_types::ExtendedHeader;
use celestia_types::eds::{EdsId, ExtendedDataSquare};
use celestia_types::namespace_data::{NamespaceData, NamespaceDataId};
use celestia_types::nmt::Namespace;
use celestia_types::row::{Row, RowId};
use celestia_types::sample::{Sample, SampleId};
use futures::future::BoxFuture;
use futures::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use futures::stream::{FuturesUnordered, StreamExt};
use integer_encoding::VarInt;
use libp2p::{PeerId, StreamProtocol};
use lumina_utils::time::{Interval, timeout};
use prost::Message;
use rand::seq::SliceRandom;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::warn;

// TODO: fix path in celestia-node repo
use celestia_proto::{Response as ProtoResponse, Status as ProtoStatus};

use crate::p2p::P2pError;
use crate::p2p::shrex::codec::{CodecError, RequestCodec, ResponseCodec};
use crate::p2p::shrex::{
    Config, EDS_PROTOCOL_ID, Event, NAMESPACE_DATA_PROTOCOL_ID, ROW_PROTOCOL_ID, Result,
    SAMPLE_PROTOCOL_ID, ShrExError,
};
use crate::p2p::utils::OneshotSender;
use crate::peer_tracker::PeerTracker;
use crate::store::Store;
use crate::store::StoreError;
use crate::utils::protocol_id;

const MAX_PEERS: usize = 10;
const SCHEDULE_PENDING_INTERVAL: Duration = Duration::from_millis(100);
const STATUS_MAX_SIZE: usize = 16;
const MAX_TRIES: usize = 5;

const OPEN_STREAM_TIMEOUT: Duration = Duration::from_secs(1);
const SEND_REQ_TIMEOUT: Duration = Duration::from_secs(1);
const RECV_RESP_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) struct Client<S>
where
    S: Store + 'static,
{
    stream_ctrl: libp2p_stream::Control,
    network_id: String,
    store: Arc<S>,
    next_req_id: u64,
    cancellation_token: CancellationToken,
    pending_reqs: VecDeque<Request>,
    ongoing_reqs: HashMap<u64, Request>,
    ongoing_reqs_tasks:
        FuturesUnordered<BoxFuture<'static, Option<(u64, Result<Vec<u8>, RequestError>)>>>,
    schedule_pending_interval: Option<Interval>,
}

enum Request {
    Row(State<RowId, Row>),
    Sample(State<SampleId, Sample>),
    NamespaceData(State<NamespaceDataId, NamespaceData>),
    Eds(State<EdsId, ExtendedDataSquare>),
}

struct State<TReq, TResp>
where
    TReq: RequestCodec,
    TResp: ResponseCodec<Request = TReq>,
{
    req: TReq,
    header: ExtendedHeader,
    respond_to: OneshotSender<TResp>,
    tries_left: usize,
}

#[derive(Debug, thiserror::Error)]
enum ClientError {
    #[error("Request error: {0}")]
    Request(#[from] RequestError),
    #[error("Codec error: {0}")]
    Codec(#[from] CodecError),
}

#[derive(Debug, thiserror::Error)]
enum RequestError {
    #[error("Requested ID not found")]
    NotFound,
    #[error("Internal error on the remote peer")]
    RemoteInternalError,
    #[error("Invalid status: {0}")]
    InvalidStatus(i32),
    #[error("Send request timed out")]
    SendRequestTimedOut,
    #[error("Recieve response timed out")]
    ReceiveResponseTimedOut,
    #[error("Open stream timed out")]
    OpenStreamTimedOut,
    #[error("Failed to open stream: {0}")]
    OpenStreamFailed(#[from] libp2p_stream::OpenStreamError),
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

impl<TReq, TResp> State<TReq, TResp>
where
    TReq: RequestCodec,
    TResp: ResponseCodec<Request = TReq>,
{
    fn can_retry(&self) -> bool {
        self.tries_left > 0 && !self.respond_to.is_closed()
    }

    fn decrease_tries(&mut self) {
        self.tries_left = self.tries_left.saturating_sub(1)
    }

    fn decode_verify_respond(&mut self, raw_data: &[u8]) -> Result<(), CodecError> {
        let resp = TResp::decode_and_verify(raw_data, &self.req, &self.header)?;
        self.respond_to.maybe_send_ok(resp);
        Ok(())
    }
}

impl Request {
    fn protocol_id(&self, network_id: &str) -> StreamProtocol {
        match self {
            Request::Row(_) => protocol_id(network_id, ROW_PROTOCOL_ID),
            Request::Sample(_) => protocol_id(network_id, SAMPLE_PROTOCOL_ID),
            Request::NamespaceData(_) => protocol_id(network_id, NAMESPACE_DATA_PROTOCOL_ID),
            Request::Eds(_) => protocol_id(network_id, EDS_PROTOCOL_ID),
        }
    }

    fn encode(&self) -> Vec<u8> {
        match self {
            Request::Row(state) => RequestCodec::encode(&state.req),
            Request::Sample(state) => RequestCodec::encode(&state.req),
            Request::NamespaceData(state) => RequestCodec::encode(&state.req),
            Request::Eds(state) => RequestCodec::encode(&state.req),
        }
    }

    fn decrease_tries(&mut self) {
        match self {
            Request::Row(state) => state.decrease_tries(),
            Request::Sample(state) => state.decrease_tries(),
            Request::NamespaceData(state) => state.decrease_tries(),
            Request::Eds(state) => state.decrease_tries(),
        }
    }

    fn can_retry(&mut self) -> bool {
        match self {
            Request::Row(state) => state.can_retry(),
            Request::Sample(state) => state.can_retry(),
            Request::NamespaceData(state) => state.can_retry(),
            Request::Eds(state) => state.can_retry(),
        }
    }

    fn decode_verify_respond(&mut self, raw_data: &[u8]) -> Result<(), CodecError> {
        match self {
            Request::Row(state) => state.decode_verify_respond(raw_data),
            Request::Sample(state) => state.decode_verify_respond(raw_data),
            Request::NamespaceData(state) => state.decode_verify_respond(raw_data),
            Request::Eds(state) => state.decode_verify_respond(raw_data),
        }
    }

    fn respond_channel_is_closed(&self) -> bool {
        match self {
            Request::Row(state) => state.respond_to.is_closed(),
            Request::Sample(state) => state.respond_to.is_closed(),
            Request::NamespaceData(state) => state.respond_to.is_closed(),
            Request::Eds(state) => state.respond_to.is_closed(),
        }
    }
}

impl<S> Client<S>
where
    S: Store,
{
    pub(super) fn new(config: &Config<'_, S>, stream_ctrl: libp2p_stream::Control) -> Client<S> {
        Client {
            stream_ctrl,
            network_id: config.network_id.to_owned(),
            store: config.header_store.clone(),
            next_req_id: 0,
            cancellation_token: CancellationToken::new(),
            pending_reqs: VecDeque::new(),
            ongoing_reqs: HashMap::new(),
            ongoing_reqs_tasks: FuturesUnordered::new(),
            schedule_pending_interval: None,
        }
    }

    async fn common_req_init<T>(
        &mut self,
        height: u64,
        respond_to: oneshot::Sender<Result<T, P2pError>>,
    ) -> Option<(OneshotSender<T>, ExtendedHeader)> {
        let mut respond_to = OneshotSender::new(respond_to, ShrExError::RequestCancelled);

        if self.cancellation_token.is_cancelled() {
            respond_to.maybe_send_err(ShrExError::RequestCancelled);
            return None;
        }

        let header = match get_header(&*self.store, height).await {
            Ok(header) => header,
            Err(e) => {
                respond_to.maybe_send_err(e);
                return None;
            }
        };

        Some((respond_to, header))
    }

    pub(super) async fn get_row(
        &mut self,
        height: u64,
        index: u16,
        respond_to: oneshot::Sender<Result<Row, P2pError>>,
    ) {
        let Some((mut respond_to, header)) = self.common_req_init(height, respond_to).await else {
            return;
        };

        let row_id = match RowId::new(index, height) {
            Ok(row_id) => row_id,
            Err(e) => return respond_to.maybe_send_err(ShrExError::invalid_request(e)),
        };

        self.pending_reqs.push_back(Request::Row(State {
            req: row_id,
            header,
            respond_to,
            tries_left: MAX_TRIES,
        }));
    }

    pub(super) async fn get_sample(
        &mut self,
        height: u64,
        row_index: u16,
        column_index: u16,
        respond_to: oneshot::Sender<Result<Sample, P2pError>>,
    ) {
        let Some((mut respond_to, header)) = self.common_req_init(height, respond_to).await else {
            return;
        };

        let sample_id = match SampleId::new(row_index, column_index, height) {
            Ok(sample_id) => sample_id,
            Err(e) => return respond_to.maybe_send_err(ShrExError::invalid_request(e)),
        };

        self.pending_reqs.push_back(Request::Sample(State {
            req: sample_id,
            header,
            respond_to,
            tries_left: MAX_TRIES,
        }));
    }

    pub(super) async fn get_namespace_data(
        &mut self,
        height: u64,
        namespace: Namespace,
        respond_to: oneshot::Sender<Result<NamespaceData, P2pError>>,
    ) {
        let Some((mut respond_to, header)) = self.common_req_init(height, respond_to).await else {
            return;
        };

        let nd_id = match NamespaceDataId::new(namespace, height) {
            Ok(nd_id) => nd_id,
            Err(e) => return respond_to.maybe_send_err(ShrExError::invalid_request(e)),
        };

        self.pending_reqs.push_back(Request::NamespaceData(State {
            req: nd_id,
            header,
            respond_to,
            tries_left: MAX_TRIES,
        }));
    }

    pub(super) async fn get_eds(
        &mut self,
        height: u64,
        respond_to: oneshot::Sender<Result<ExtendedDataSquare, P2pError>>,
    ) {
        let Some((mut respond_to, header)) = self.common_req_init(height, respond_to).await else {
            return;
        };

        let eds_id = match EdsId::new(height) {
            Ok(eds_id) => eds_id,
            Err(e) => return respond_to.maybe_send_err(ShrExError::invalid_request(e)),
        };

        self.pending_reqs.push_back(Request::Eds(State {
            req: eds_id,
            header,
            respond_to,
            tries_left: MAX_TRIES,
        }));
    }

    fn has_pending_requests(&self) -> bool {
        !self.pending_reqs.is_empty()
    }

    pub(super) fn schedule_pending_requests(&mut self, peer_tracker: &PeerTracker) {
        if self.cancellation_token.is_cancelled() || self.pending_reqs.is_empty() {
            return;
        }

        let mut peers = peer_tracker
            .peers()
            .filter(|peer| peer.is_full() && peer.is_connected() && peer.is_trusted())
            .collect::<Vec<_>>();

        if !peers.is_empty() {
            // TODO: We can add a parameter for what kind of sorting we want for the peers.
            // For example we can sort by peer scoring or by ping latency etc.
            peers.shuffle(&mut rand::thread_rng());
            peers.truncate(MAX_PEERS);

            for (i, mut req) in self
                .pending_reqs
                .drain(..)
                // We filter before enumerate, just for keeping `i` correct
                .filter(|req| !req.respond_channel_is_closed())
                .enumerate()
            {
                // Choose different peer for each request
                let peer = peers[i % peers.len()];
                let peer_id = *peer.id();

                let req_id = get_next_req_id(&mut self.next_req_id);
                let stream_ctrl = self.stream_ctrl.clone();
                let raw_req = req.encode();
                let protocol_id = req.protocol_id(&self.network_id);
                let cancellation_token = self.cancellation_token.clone();

                self.ongoing_reqs_tasks.push(Box::pin(
                    cancellation_token.run_until_cancelled_owned(async move {
                        let res =
                            request_response_task(stream_ctrl, peer_id, raw_req, protocol_id).await;

                        (req_id, res)
                    }),
                ));

                req.decrease_tries();
                self.ongoing_reqs.insert(req_id, req);
            }
        }
    }

    pub(super) fn on_stop(&mut self) {
        self.cancellation_token.cancel();
        self.pending_reqs.clear();
        self.ongoing_reqs.clear();
        self.schedule_pending_interval.take();
    }

    fn on_result(&mut self, req_id: u64, res: Result<Vec<u8>, RequestError>) {
        let Some(mut req) = self.ongoing_reqs.remove(&req_id) else {
            return;
        };

        let raw_data = match res {
            Ok(raw_data) => raw_data,
            Err(e) => return self.on_error(req, e.into()),
        };

        if let Err(e) = req.decode_verify_respond(&raw_data) {
            self.on_error(req, e.into());
        }
    }

    fn on_error(&mut self, mut req: Request, error: ClientError) {
        warn!("shrex error: {error}");

        req.decrease_tries();

        if req.can_retry() {
            // move failed request to pending
            self.pending_reqs.push_back(req);
        }
    }

    pub(super) fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Event> {
        while let Poll::Ready(Some(opt)) = self.ongoing_reqs_tasks.poll_next_unpin(cx) {
            // `None` is returned on cancellation, we should still continue polling.
            if let Some((req_id, res)) = opt {
                self.on_result(req_id, res);
            }
        }

        // If we have pending requests then initialize interval.
        //
        // We use this mechanism to give some buffer for more requests to
        // be accumulated and avoid calling `schedule_pending_requests` on
        // each iteration.
        if self.schedule_pending_interval.is_none() && self.has_pending_requests() {
            self.schedule_pending_interval = Some(Interval::new(SCHEDULE_PENDING_INTERVAL));
        }

        if let Some(interval) = self.schedule_pending_interval.as_mut()
            && interval.poll_tick(cx).is_ready()
        {
            return Poll::Ready(Event::SchedulePendingRequests);
        }

        Poll::Pending
    }
}

async fn get_header(store: &impl Store, height: u64) -> Result<ExtendedHeader, P2pError> {
    match store.get_by_height(height).await {
        Ok(header) => Ok(header),
        Err(StoreError::NotFound) => {
            let pruned_ranges = store.get_pruned_ranges().await?;

            if pruned_ranges.contains(height) {
                Err(P2pError::HeaderPruned(height))
            } else {
                Err(P2pError::HeaderNotSynced(height))
            }
        }
        Err(e) => Err(e.into()),
    }
}

fn get_next_req_id(next_req_id: &mut u64) -> u64 {
    let req_id = *next_req_id;
    *next_req_id = next_req_id.saturating_sub(1);
    req_id
}

async fn request_response_task(
    mut stream_ctrl: libp2p_stream::Control,
    peer_id: PeerId,
    raw_req: Vec<u8>,
    protocol_id: StreamProtocol,
) -> Result<Vec<u8>, RequestError> {
    // We have a lower timeout on just opening the stream in order to retry
    // quicker with another peer when remote peer has some network issues.
    let mut stream = timeout(OPEN_STREAM_TIMEOUT, async {
        stream_ctrl.open_stream(peer_id, protocol_id).await
    })
    .await
    .map_err(|_| RequestError::OpenStreamTimedOut)??;

    timeout(SEND_REQ_TIMEOUT, async {
        stream.write_all(&raw_req[..]).await?;
        stream.flush().await?;
        // This closes the write end only.
        stream.close().await
    })
    .await
    .map_err(|_| RequestError::SendRequestTimedOut)??;

    let (status, data) = timeout(RECV_RESP_TIMEOUT, async {
        let status = read_status(&mut stream).await?;
        let mut data = Vec::new();

        if status == ProtoStatus::Ok as i32 {
            stream.read_to_end(&mut data).await?;
        }

        Result::<_, RequestError>::Ok((status, data))
    })
    .await
    .map_err(|_| RequestError::ReceiveResponseTimedOut)??;

    match ProtoStatus::try_from(status) {
        Ok(ProtoStatus::Ok) => Ok(data),
        Ok(ProtoStatus::NotFound) => Err(RequestError::NotFound),
        Ok(ProtoStatus::Internal) => Err(RequestError::RemoteInternalError),
        Ok(ProtoStatus::Invalid) => Err(RequestError::InvalidStatus(status)),
        Err(prost::UnknownEnumValue(val)) => Err(RequestError::InvalidStatus(val)),
    }
}

async fn read_varint<T>(io: &mut T) -> io::Result<usize>
where
    T: AsyncRead + Unpin + Send,
{
    let mut buf = [0u8; 10];

    for i in 0..buf.len() {
        io.read_exact(&mut buf[i..=i]).await?;

        if let Some((val, _)) = usize::decode_var(&buf[..=i]) {
            return Ok(val);
        }
    }

    Err(io::Error::other("shrex: failed to read valid varint"))
}

async fn read_status<T>(io: &mut T) -> io::Result<i32>
where
    T: AsyncRead + Unpin + Send,
{
    let len = read_varint(io).await?;

    if len > STATUS_MAX_SIZE {
        let s = format!("shrex: status message bigger than STATUS_MAX_SIZE");
        return Err(io::Error::other(s));
    }

    let mut buf = vec![0u8; len];
    io.read_exact(&mut buf[..]).await?;

    let resp = ProtoResponse::decode(&buf[..]).map_err(|e| {
        let s = format!("shrex: failed to decode `Response`: {e}");
        io::Error::other(s)
    })?;

    Ok(resp.status)
}

async fn write_status<T>(io: &mut T, status: ProtoStatus) -> io::Result<()>
where
    T: AsyncWrite + Unpin + Send,
{
    let resp = ProtoResponse {
        status: status as i32,
    };

    let data = resp.encode_to_vec();
    let varint = data.len().encode_var_vec();

    io.write_all(&varint).await?;
    io.write_all(&data).await?;

    Ok(())
}
