use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use celestia_proto::share::p2p::shrex::{Response as ProtoResponse, Status as ProtoStatus};
use celestia_types::eds::{EdsId, ExtendedDataSquare};
use celestia_types::namespace_data::{NamespaceData, NamespaceDataId};
use celestia_types::nmt::Namespace;
use celestia_types::row::{Row, RowId};
use celestia_types::sample::{Sample, SampleId};
use celestia_types::{AppVersion, AxisType, DataAvailabilityHeader, ExtendedHeader};
use futures::future::BoxFuture;
use futures::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use futures::stream::{FuturesUnordered, StreamExt};
use integer_encoding::VarInt;
use libp2p::{PeerId, StreamProtocol};
use lumina_utils::time::{Interval, timeout};
use prost::Message;
use rand::seq::SliceRandom;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::p2p::P2pError;
use crate::p2p::shrex::codec::{CodecError, RequestCodec, ResponseCodec};
use crate::p2p::shrex::pool_tracker::PoolTracker;
use crate::p2p::shrex::{
    Config, EDS_PROTOCOL_ID, EMPTY_EDS, EMPTY_EDS_DAH, Event, NAMESPACE_DATA_PROTOCOL_ID,
    ROW_PROTOCOL_ID, Result, SAMPLE_PROTOCOL_ID, ShrExError,
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

type OngoingReqTaskResult = (u64, Result<Vec<u8>, RequestError>);

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
    ongoing_reqs_tasks: FuturesUnordered<BoxFuture<'static, Option<OngoingReqTaskResult>>>,
    schedule_pending_interval: Option<Interval>,
}

struct Request {
    ctx: RequestContext,
    dah: DataAvailabilityHeader,
    app_version: AppVersion,
    tries_left: usize,
    cancellation_token: CancellationToken,
}

enum RequestContext {
    Row(GenericRequestContext<RowId, Row>),
    Sample(GenericRequestContext<SampleId, Sample>),
    NamespaceData(GenericRequestContext<NamespaceDataId, NamespaceData>),
    Eds(GenericRequestContext<EdsId, ExtendedDataSquare>),
}

struct GenericRequestContext<TReq, TResp>
where
    TReq: RequestCodec,
    TResp: ResponseCodec<Request = TReq>,
{
    req: TReq,
    respond_to: OneshotSender<TResp>,
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

impl<S> Client<S>
where
    S: Store,
{
    pub(super) fn new(config: &Config<'_, S>) -> Client<S> {
        Client {
            stream_ctrl: config.stream_ctrl.clone(),
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

        if index >= header.dah.square_width() {
            respond_to.maybe_send_err(ShrExError::invalid_request("`index` out of bound"));
            return;
        }

        if header.dah == *EMPTY_EDS_DAH {
            let shares = EMPTY_EDS.row(index).expect("coordinates already checked");
            respond_to.maybe_send_ok(Row { shares });
            return;
        }

        let row_id = match RowId::new(index, height) {
            Ok(row_id) => row_id,
            Err(e) => return respond_to.maybe_send_err(ShrExError::invalid_request(e)),
        };

        self.pending_reqs.push_back(Request {
            ctx: RequestContext::Row(GenericRequestContext {
                req: row_id,
                respond_to,
            }),
            app_version: header.app_version().expect("header validated"),
            dah: header.dah,
            tries_left: MAX_TRIES,
            cancellation_token: self.cancellation_token.child_token(),
        });
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

        if row_index >= header.dah.square_width() {
            respond_to.maybe_send_err(ShrExError::invalid_request("`row_index` out of bound"));
            return;
        }

        if column_index >= header.dah.square_width() {
            respond_to.maybe_send_err(ShrExError::invalid_request("`column_index` out of bound"));
            return;
        }

        if header.dah == *EMPTY_EDS_DAH {
            let sample = Sample::new(row_index, column_index, AxisType::Row, &EMPTY_EDS)
                .expect("coordinates already checked");
            respond_to.maybe_send_ok(sample);
            return;
        }

        let sample_id = match SampleId::new(row_index, column_index, height) {
            Ok(sample_id) => sample_id,
            Err(e) => return respond_to.maybe_send_err(ShrExError::invalid_request(e)),
        };

        self.pending_reqs.push_back(Request {
            ctx: RequestContext::Sample(GenericRequestContext {
                req: sample_id,
                respond_to,
            }),
            app_version: header.app_version().expect("header validated"),
            dah: header.dah,
            tries_left: MAX_TRIES,
            cancellation_token: self.cancellation_token.child_token(),
        });
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

        if header.dah == *EMPTY_EDS_DAH {
            let rows = EMPTY_EDS
                .get_namespace_data(namespace, &EMPTY_EDS_DAH, height)
                .expect("invalid eds or dah")
                .into_iter()
                .map(|(_, row)| row)
                .collect();
            respond_to.maybe_send_ok(NamespaceData::new(rows));
            return;
        }

        let nd_id = match NamespaceDataId::new(namespace, height) {
            Ok(nd_id) => nd_id,
            Err(e) => return respond_to.maybe_send_err(ShrExError::invalid_request(e)),
        };

        self.pending_reqs.push_back(Request {
            ctx: RequestContext::NamespaceData(GenericRequestContext {
                req: nd_id,
                respond_to,
            }),
            app_version: header.app_version().expect("header validated"),
            dah: header.dah,
            tries_left: MAX_TRIES,
            cancellation_token: self.cancellation_token.child_token(),
        });
    }

    pub(super) async fn get_eds(
        &mut self,
        height: u64,
        respond_to: oneshot::Sender<Result<ExtendedDataSquare, P2pError>>,
    ) {
        let Some((mut respond_to, header)) = self.common_req_init(height, respond_to).await else {
            return;
        };

        if header.dah == *EMPTY_EDS_DAH {
            respond_to.maybe_send_ok(EMPTY_EDS.clone());
            return;
        }

        let eds_id = match EdsId::new(height) {
            Ok(eds_id) => eds_id,
            Err(e) => return respond_to.maybe_send_err(ShrExError::invalid_request(e)),
        };

        self.pending_reqs.push_back(Request {
            ctx: RequestContext::Eds(GenericRequestContext {
                req: eds_id,
                respond_to,
            }),
            app_version: header.app_version().expect("header validated"),
            dah: header.dah,
            tries_left: MAX_TRIES,
            cancellation_token: self.cancellation_token.child_token(),
        });
    }

    fn has_pending_requests(&self) -> bool {
        !self.pending_reqs.is_empty()
    }

    pub(super) fn schedule_pending_requests(
        &mut self,
        peer_tracker: &PeerTracker,
        pool_tracker: &PoolTracker<S>,
    ) {
        if self.cancellation_token.is_cancelled() || self.pending_reqs.is_empty() {
            return;
        }

        // TODO: incorporate pool_tracker
        //
        // The following scenarios need be handled:
        //
        // 1. User requests a shrex ID and we have validated pool for it
        // 2. User requests a shrex ID and we have validated pool but it is empty
        // 3. User requests a shrex ID for a block from the future.
        //    In this case:
        //      * Pool tracker may have pool in Caditates. We need to skip this request
        //        and retry later when the pool is validated.
        //      * The block maybe is too far to the future an pool tracker doesn't even
        //        have it as candidate. What should we do in this case? Wait for the
        //        height or return an error? One option is to wait forever and user needs
        //        to use the `timeout` parameter in `p2p.rs`. (NOTE: `timeout` in `p2p.rs`
        //        is not related to shrex recv timeout, the `timeout` in `p2p.rs` is the timeout
        //        for the whole action and shrex nows nothing about it).
        // 4. User requests a shrex ID from the past. In this case we can we need to
        //    send the request to any connected full node.
        //

        let mut peers = peer_tracker
            .peers()
            // TODO
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
                .filter(|req| !req.is_respond_channel_closed())
                .enumerate()
            {
                // Choose different peer for each request
                let peer = peers[i % peers.len()];
                let peer_id = *peer.id();

                let req_id = get_next_req_id(&mut self.next_req_id);
                let stream_ctrl = self.stream_ctrl.clone();
                let raw_req = req.encode();
                let protocol_id = req.protocol_id(&self.network_id);
                let cancellation_token = req.cancellation_token.clone();

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

        if req.can_retry() {
            // move failed request to pending
            self.pending_reqs.push_back(req);
        } else {
            req.respond_with_error(ShrExError::MaxTriesReached);
        }
    }

    pub(super) fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Event> {
        // Any tasks associeted with the request will be canceled because of
        // `cancellation_token.cacel()` in `Request::drop`.
        self.pending_reqs
            .retain_mut(|req| !req.poll_respond_channel_closed(cx).is_ready());
        self.ongoing_reqs
            .retain(|_, req| !req.poll_respond_channel_closed(cx).is_ready());

        while let Poll::Ready(Some(opt)) = self.ongoing_reqs_tasks.poll_next_unpin(cx) {
            // When a task is cancelled via its `cancellation_token`, then `None` is returned.
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

impl<TReq, TResp> GenericRequestContext<TReq, TResp>
where
    TReq: RequestCodec,
    TResp: ResponseCodec<Request = TReq>,
{
    fn decode_verify_respond(
        &mut self,
        raw_data: &[u8],
        dah: &DataAvailabilityHeader,
        app_version: AppVersion,
    ) -> Result<(), CodecError> {
        let resp = TResp::decode_and_verify(raw_data, &self.req, dah, app_version)?;
        self.respond_to.maybe_send_ok(resp);
        Ok(())
    }
}

impl Request {
    fn protocol_id(&self, network_id: &str) -> StreamProtocol {
        match &self.ctx {
            RequestContext::Row(_) => protocol_id(network_id, ROW_PROTOCOL_ID),
            RequestContext::Sample(_) => protocol_id(network_id, SAMPLE_PROTOCOL_ID),
            RequestContext::NamespaceData(_) => protocol_id(network_id, NAMESPACE_DATA_PROTOCOL_ID),
            RequestContext::Eds(_) => protocol_id(network_id, EDS_PROTOCOL_ID),
        }
    }

    fn encode(&self) -> Vec<u8> {
        match &self.ctx {
            RequestContext::Row(ctx) => RequestCodec::encode(&ctx.req),
            RequestContext::Sample(ctx) => RequestCodec::encode(&ctx.req),
            RequestContext::NamespaceData(ctx) => RequestCodec::encode(&ctx.req),
            RequestContext::Eds(ctx) => RequestCodec::encode(&ctx.req),
        }
    }

    fn decrease_tries(&mut self) {
        self.tries_left = self.tries_left.saturating_sub(1)
    }

    fn can_retry(&self) -> bool {
        self.tries_left > 0 && !self.is_respond_channel_closed()
    }

    fn decode_verify_respond(&mut self, raw_data: &[u8]) -> Result<(), CodecError> {
        match &mut self.ctx {
            RequestContext::Row(state) => {
                state.decode_verify_respond(raw_data, &self.dah, self.app_version)
            }
            RequestContext::Sample(state) => {
                state.decode_verify_respond(raw_data, &self.dah, self.app_version)
            }
            RequestContext::NamespaceData(state) => {
                state.decode_verify_respond(raw_data, &self.dah, self.app_version)
            }
            RequestContext::Eds(state) => {
                state.decode_verify_respond(raw_data, &self.dah, self.app_version)
            }
        }
    }

    fn respond_with_error(&mut self, error: impl Into<P2pError>) {
        match &mut self.ctx {
            RequestContext::Row(ctx) => ctx.respond_to.maybe_send_err(error),
            RequestContext::Sample(ctx) => ctx.respond_to.maybe_send_err(error),
            RequestContext::NamespaceData(ctx) => ctx.respond_to.maybe_send_err(error),
            RequestContext::Eds(ctx) => ctx.respond_to.maybe_send_err(error),
        }
    }

    fn is_respond_channel_closed(&self) -> bool {
        match &self.ctx {
            RequestContext::Row(ctx) => ctx.respond_to.is_closed(),
            RequestContext::Sample(ctx) => ctx.respond_to.is_closed(),
            RequestContext::NamespaceData(ctx) => ctx.respond_to.is_closed(),
            RequestContext::Eds(ctx) => ctx.respond_to.is_closed(),
        }
    }

    fn poll_respond_channel_closed(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        match &mut self.ctx {
            RequestContext::Row(ctx) => ctx.respond_to.poll_closed(cx),
            RequestContext::Sample(ctx) => ctx.respond_to.poll_closed(cx),
            RequestContext::NamespaceData(ctx) => ctx.respond_to.poll_closed(cx),
            RequestContext::Eds(ctx) => ctx.respond_to.poll_closed(cx),
        }
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        self.cancellation_token.cancel();
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
            // NOTE: We could limit the receiving size but we don't,
            // as celestia's blocks keep growing, so big responses are expected
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

    Err(io::Error::other("failed to read valid varint"))
}

async fn read_status<T>(io: &mut T) -> io::Result<i32>
where
    T: AsyncRead + Unpin + Send,
{
    let len = read_varint(io).await?;

    if len > STATUS_MAX_SIZE {
        return Err(io::Error::other(
            "status message bigger than STATUS_MAX_SIZE",
        ));
    }

    let mut buf = vec![0u8; len];
    io.read_exact(&mut buf[..]).await?;

    let resp = ProtoResponse::decode(&buf[..]).map_err(|e| {
        let s = format!("failed to decode `Response`: {e}");
        io::Error::other(s)
    })?;

    Ok(resp.status)
}
