use std::cmp::Reverse;
use std::collections::{HashMap, VecDeque};
use std::fmt::{self, Debug};
use std::hash::Hash;
use std::task::{Context, Poll};
use std::time::Duration;

use celestia_proto::p2p::pb::header_request::Data;
use celestia_proto::p2p::pb::{HeaderRequest, HeaderResponse};
use celestia_types::ExtendedHeader;
use futures::future::{BoxFuture, FutureExt, join_all};
use futures::stream::{FuturesUnordered, StreamExt};
use libp2p::PeerId;
use libp2p::request_response::{OutboundFailure, OutboundRequestId};
use lumina_utils::executor::yield_now;
use lumina_utils::time::{Instant, Interval};
use rand::seq::SliceRandom;
use smallvec::SmallVec;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tracing::{debug, instrument, trace};

use crate::p2p::P2pError;
use crate::p2p::header_ex::utils::{HeaderRequestExt, HeaderResponseExt};
use crate::p2p::header_ex::{Event, HeaderExError, ReqRespBehaviour};
use crate::peer_tracker::PeerTracker;
use crate::utils::OneshotResultSender;

const MAX_PEERS: usize = 10;
const MAX_TRIES: usize = 3;
const SCHEDULE_PENDING_INTERVAL: Duration = Duration::from_millis(100);
const SEND_NEED_MORE_PEERS_AFTER: Duration = Duration::from_secs(60);

pub(super) struct HeaderExClientHandler<S = ReqRespBehaviour>
where
    S: RequestSender,
{
    reqs: HashMap<S::RequestId, State>,
    head_reqs: VecDeque<OneshotSender>,
    head_req_scheduled: bool,
    pending_reqs: HashMap<PeerKind, VecDeque<State>>,
    cancellation_token: CancellationToken,
    tasks: FuturesUnordered<BoxFuture<'static, TaskResult<S::RequestId>>>,
    events: VecDeque<Event>,
    schedule_pending_interval: Option<Interval>,
    last_need_trusted_sent: Option<Instant>,
    last_need_archival_sent: Option<Instant>,
}

enum TaskResult<ReqId> {
    Req(ReqId, Result<Vec<ExtendedHeader>, HeaderExError>),
    Head(Option<Box<ExtendedHeader>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PeerKind {
    Any,
    Archival,
    Trusted,
    TrustedArchival,
}

#[derive(Debug)]
struct State {
    peer_kind: PeerKind,
    request: HeaderRequest,
    respond_to: OneshotSender,
    tries_left: usize,
}

/// Oneshot sender that responds with `RequestCancelled` if not used.
struct OneshotSender(Option<OneshotResultSender<Vec<ExtendedHeader>, P2pError>>);

pub(super) trait RequestSender {
    type RequestId: Clone + Copy + Hash + Eq + Debug + Send + Sync + 'static;

    fn send_request(&mut self, peer: &PeerId, request: HeaderRequest) -> Self::RequestId;
}

impl RequestSender for ReqRespBehaviour {
    type RequestId = OutboundRequestId;

    fn send_request(&mut self, peer: &PeerId, request: HeaderRequest) -> OutboundRequestId {
        self.send_request(peer, request)
    }
}

impl OneshotSender {
    fn new(tx: oneshot::Sender<Result<Vec<ExtendedHeader>, P2pError>>) -> Self {
        OneshotSender(Some(tx))
    }

    fn is_closed(&self) -> bool {
        self.0.as_ref().is_none_or(|tx| tx.is_closed())
    }

    fn maybe_send(&mut self, result: Result<Vec<ExtendedHeader>, P2pError>) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(result);
        }
    }

    fn maybe_send_ok(&mut self, val: Vec<ExtendedHeader>) {
        self.maybe_send(Ok(val));
    }

    fn maybe_send_err<E>(&mut self, err: E)
    where
        E: Into<P2pError>,
    {
        self.maybe_send(Err(err.into()));
    }
}

impl Drop for OneshotSender {
    fn drop(&mut self) {
        // If sender is dropped without being used, then `RequestCancelled` is send.
        self.maybe_send_err(HeaderExError::RequestCancelled);
    }
}

impl Debug for OneshotSender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OneshotSender { .. }")
    }
}

impl<S> HeaderExClientHandler<S>
where
    S: RequestSender,
{
    pub(super) fn new() -> Self {
        HeaderExClientHandler {
            reqs: HashMap::new(),
            head_reqs: VecDeque::new(),
            head_req_scheduled: false,
            pending_reqs: HashMap::new(),
            cancellation_token: CancellationToken::new(),
            tasks: FuturesUnordered::new(),
            events: VecDeque::new(),
            schedule_pending_interval: None,
            last_need_trusted_sent: None,
            last_need_archival_sent: None,
        }
    }

    #[instrument(level = "trace", skip(self, respond_to))]
    pub(super) fn on_send_request(
        &mut self,
        request: HeaderRequest,
        respond_to: OneshotResultSender<Vec<ExtendedHeader>, P2pError>,
    ) {
        let mut respond_to = OneshotSender::new(respond_to);

        if self.cancellation_token.is_cancelled() {
            respond_to.maybe_send_err(HeaderExError::RequestCancelled);
            return;
        }

        if !request.is_valid() {
            respond_to.maybe_send_err(HeaderExError::InvalidRequest);
            return;
        }

        if request.is_head_request() {
            self.head_reqs.push_back(respond_to);
        } else {
            self.pending_reqs
                .entry(PeerKind::Any)
                .or_default()
                .push_back(State {
                    peer_kind: PeerKind::Any,
                    request,
                    respond_to,
                    tries_left: MAX_TRIES,
                });
        }
    }

    fn has_pending_head_requests(&self) -> bool {
        !self.head_reqs.is_empty() && !self.head_req_scheduled
    }

    fn has_pending_requests(&self) -> bool {
        self.has_pending_head_requests() || self.pending_reqs.values().any(|reqs| !reqs.is_empty())
    }

    fn needs_trusted_peers(&self) -> bool {
        self.has_pending_head_requests()
            || self
                .pending_reqs
                .get(&PeerKind::Trusted)
                .is_some_and(|reqs| !reqs.is_empty())
            || self
                .pending_reqs
                .get(&PeerKind::TrustedArchival)
                .is_some_and(|reqs| !reqs.is_empty())
    }

    fn needs_archival_peers(&self) -> bool {
        self.pending_reqs
            .get(&PeerKind::Archival)
            .is_some_and(|reqs| !reqs.is_empty())
            || self
                .pending_reqs
                .get(&PeerKind::TrustedArchival)
                .is_some_and(|reqs| !reqs.is_empty())
    }

    pub(super) fn schedule_pending_requests(&mut self, sender: &mut S, peer_tracker: &PeerTracker) {
        if self.has_pending_head_requests() {
            self.schedule_head_request(sender, peer_tracker);
        }

        self.schedule_pending_requests_impl(sender, peer_tracker, PeerKind::Trusted);
        self.schedule_pending_requests_impl(sender, peer_tracker, PeerKind::TrustedArchival);
        self.schedule_pending_requests_impl(sender, peer_tracker, PeerKind::Archival);
        self.schedule_pending_requests_impl(sender, peer_tracker, PeerKind::Any);

        // If all pending requests were scheduled then disable interval.
        if !self.has_pending_requests() {
            self.schedule_pending_interval.take();
        }

        // Check every `SEND_NEED_MORE_PEERS_AFTER` seconds if trusted peers are needed
        // and generate `Event::NeedTrustedPeers`
        if self
            .last_need_trusted_sent
            .is_none_or(|tm| tm.elapsed() >= SEND_NEED_MORE_PEERS_AFTER)
            && self.needs_trusted_peers()
        {
            self.events.push_back(Event::NeedTrustedPeers);
            self.last_need_trusted_sent = Some(Instant::now());
        }

        // Check every `SEND_NEED_MORE_PEERS_AFTER` seconds if archival peers are needed
        // and generate `Event::NeedArchivalPeers`
        if self
            .last_need_archival_sent
            .is_none_or(|tm| tm.elapsed() >= SEND_NEED_MORE_PEERS_AFTER)
            && self.needs_archival_peers()
        {
            self.events.push_back(Event::NeedArchivalPeers);
            self.last_need_archival_sent = Some(Instant::now());
        }
    }

    fn schedule_pending_requests_impl(
        &mut self,
        sender: &mut S,
        peer_tracker: &PeerTracker,
        peer_kind: PeerKind,
    ) {
        let Some(pending_reqs) = self.pending_reqs.get_mut(&peer_kind) else {
            return;
        };

        if pending_reqs.is_empty() {
            return;
        }

        let mut peers = peer_tracker
            .peers()
            .filter(|peer| match peer_kind {
                PeerKind::Any => peer.is_connected(),
                PeerKind::Archival => peer.is_connected() && peer.is_archival(),
                PeerKind::Trusted => peer.is_connected() && peer.is_trusted(),
                PeerKind::TrustedArchival => {
                    peer.is_connected() && peer.is_trusted() && peer.is_archival()
                }
            })
            .collect::<SmallVec<[_; MAX_PEERS]>>();

        if !peers.is_empty() {
            // TODO: We can add a parameter for what kind of sorting we want for the peers.
            // For example we can sort by peer scoring or by ping latency etc.
            peers.shuffle(&mut rand::thread_rng());
            peers.truncate(MAX_PEERS);

            for (i, mut state) in pending_reqs
                .drain(..)
                // We filter before enumerate, just for keeping `i` correct
                .filter(|state| !state.respond_to.is_closed())
                .enumerate()
            {
                // Choose different peer for each request
                let peer = peers[i % peers.len()];
                let req_id = sender.send_request(peer.id(), state.request.clone());
                state.tries_left -= 1;
                self.reqs.insert(req_id, state);
            }
        }
    }

    fn schedule_head_request(&mut self, sender: &mut S, peer_tracker: &PeerTracker) {
        const MIN_HEAD_RESPONSES: usize = 2;

        // Remove any closed head request channels.
        self.head_reqs.retain(|tx| !tx.is_closed());

        // If we don't have any head request channels, then do nothing.
        if self.head_reqs.is_empty() {
            return;
        }

        let peers = peer_tracker
            .peers()
            // For HEAD requests we only use trusted peers
            .filter(|peer| peer.is_connected() && peer.is_trusted())
            .take(MAX_PEERS)
            .collect::<SmallVec<[_; MAX_PEERS]>>();

        if peers.is_empty() {
            return;
        }

        let mut rxs = Vec::with_capacity(peers.len());
        let request = HeaderRequest::head_request();

        for peer in peers {
            let (tx, rx) = oneshot::channel();

            let req_id = sender.send_request(peer.id(), request.clone());
            let state = State {
                peer_kind: PeerKind::Trusted,
                request: request.clone(),
                respond_to: OneshotSender::new(tx),
                tries_left: 0,
            };

            self.reqs.insert(req_id, state);
            rxs.push(rx);
        }

        self.head_req_scheduled = true;

        // Choose the best HEAD.
        //
        // Algorithm: https://github.com/celestiaorg/go-header/blob/e50090545cc7e049d2f965d2b5c773eaa4a2c0b2/p2p/exchange.go#L357-L381
        self.tasks.push(
            async move {
                let mut resps = Vec::with_capacity(rxs.len());
                let mut counter: HashMap<_, usize> = HashMap::with_capacity(rxs.len());

                for res in join_all(rxs).await {
                    // HEAD responses must have only 1 header.
                    if let Ok(Ok(mut v)) = res
                        && v.len() == 1
                    {
                        resps.append(&mut v);
                    }
                }

                // If we don't have any valid responses, we return `None` and `poll`
                // will reschedule a new head request.
                if resps.is_empty() {
                    return TaskResult::Head(None);
                }

                // Count peers per response
                for resp in &resps {
                    *counter.entry(resp.hash()).or_default() += 1;
                }

                // Sort by height and then peers in descending order
                resps.sort_unstable_by_key(|resp| {
                    let num_of_peers = counter[&resp.hash()];
                    Reverse((resp.height(), num_of_peers))
                });

                // Return the header with the highest height that was received by at least 2 peers
                for resp in &resps {
                    if counter[&resp.hash()] >= MIN_HEAD_RESPONSES {
                        return TaskResult::Head(Some(Box::new(resp.to_owned())));
                    }
                }

                // Otherwise return the header with the maximum height
                let resp = resps.into_iter().next().expect("no responses");
                TaskResult::Head(Some(Box::new(resp)))
            }
            .boxed(),
        );
    }

    #[instrument(level = "trace", skip(self, responses), fields(responses.len = responses.len()))]
    pub(super) fn on_response_received(
        &mut self,
        peer: PeerId,
        request_id: S::RequestId,
        responses: Vec<HeaderResponse>,
    ) {
        let request = match self.reqs.get(&request_id) {
            Some(state) => state.request.to_owned(),
            None => return,
        };

        self.tasks.push(
            async move {
                let res = decode_and_verify_responses(&request, &responses).await;
                TaskResult::Req(request_id, res)
            }
            .boxed(),
        );
    }

    #[instrument(level = "debug", skip(self))]
    pub(super) fn on_failure(
        &mut self,
        peer: PeerId,
        request_id: S::RequestId,
        error: OutboundFailure,
    ) {
        debug!("Outbound failure: {error}");
        let error = HeaderExError::OutboundFailure(error);

        let Some(mut state) = self.reqs.remove(&request_id) else {
            return;
        };

        if can_retry(&state, &error) {
            let peer_kind = next_peer_kind(&state);

            self.pending_reqs
                .entry(peer_kind)
                .or_default()
                .push_back(State { peer_kind, ..state });

            return;
        }

        state.respond_to.maybe_send_err(error);
    }

    pub(super) fn on_stop(&mut self) {
        self.cancellation_token.cancel();
        self.reqs.clear();
        self.head_reqs.clear();
        self.head_req_scheduled = false;
        self.pending_reqs.clear();
        self.tasks.clear();
        self.events.clear();
        self.schedule_pending_interval.take();
    }

    pub(super) fn poll(&mut self, cx: &mut Context) -> Poll<Event> {
        if let Some(ev) = self.events.pop_front() {
            return Poll::Ready(ev);
        }

        while let Poll::Ready(Some(res)) = self.tasks.poll_next_unpin(cx) {
            match res {
                TaskResult::Head(None) => {
                    // By setting this to `false` on `None`, we trigger a retry
                    // when `schedule_pending_requests` is called.
                    self.head_req_scheduled = false;
                }
                TaskResult::Head(Some(head)) => {
                    self.head_req_scheduled = false;
                    let head = vec![*head];

                    for mut respond_to in self.head_reqs.drain(..) {
                        respond_to.maybe_send_ok(head.clone());
                    }
                }
                TaskResult::Req(req_id, res) => {
                    let Some(mut state) = self.reqs.remove(&req_id) else {
                        continue;
                    };

                    if let Err(ref e) = res
                        && can_retry(&state, e)
                    {
                        let peer_kind = next_peer_kind(&state);

                        self.pending_reqs
                            .entry(peer_kind)
                            .or_default()
                            .push_back(State { peer_kind, ..state });

                        continue;
                    }

                    let res = res.map_err(P2pError::from);
                    state.respond_to.maybe_send(res);
                }
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

fn can_retry(state: &State, err: &HeaderExError) -> bool {
    // Head requests are never retried on the level `can_retry` is called
    // but are retried only from `schedule_head_request`.
    if state.request.is_head_request() {
        return false;
    }

    if state.tries_left == 0 || state.respond_to.is_closed() {
        return false;
    }

    match err {
        HeaderExError::HeaderNotFound
        | HeaderExError::InvalidResponse
        | HeaderExError::OutboundFailure(_) => true,

        HeaderExError::InvalidRequest | HeaderExError::RequestCancelled => false,

        HeaderExError::InboundFailure(_) => {
            unreachable!("client never receives inbound connection")
        }
    }
}

fn next_peer_kind(state: &State) -> PeerKind {
    // This should never been called for head requests
    debug_assert!(!state.request.is_head_request());

    // The last try always reaches archival nodes.
    match (state.tries_left, state.peer_kind) {
        (1, PeerKind::Any) => PeerKind::Archival,
        (1, PeerKind::Trusted) => PeerKind::TrustedArchival,
        (_, peer_kind) => peer_kind,
    }
}

async fn decode_and_verify_responses(
    request: &HeaderRequest,
    responses: &[HeaderResponse],
) -> Result<Vec<ExtendedHeader>, HeaderExError> {
    trace!("Response received. Expected amount = {}", request.amount);

    if responses.is_empty() {
        return Err(HeaderExError::InvalidResponse);
    }

    let amount = usize::try_from(request.amount).expect("validated in HeaderRequestExt::is_valid");

    // Server shouldn't respond with more headers
    if responses.len() > amount {
        return Err(HeaderExError::InvalidResponse);
    }

    let mut headers = Vec::with_capacity(responses.len());

    for response in responses {
        // Unmarshal and validate. Propagate error only if nothing
        // was decoded before.
        let header = match response.to_validated_extented_header() {
            Ok(header) => header,
            Err(e) if headers.is_empty() => return Err(e),
            Err(_) => break,
        };

        trace!("Header: {header}");
        headers.push(header);

        // Validation is computation heavy so we yield on every chunk
        yield_now().await;
    }

    headers.sort_unstable_by_key(|header| header.height());

    // NOTE: Verification is done only in `get_verified_headers_range` and
    // Syncer passes the `from` parameter from Store.
    match (&request.data, headers.len()) {
        // Allow HEAD requests to have any height in their response
        (Some(Data::Origin(0)), 1) => {}

        // Make sure that starting header is the requested one and that
        // there are no gaps in the chain
        (Some(Data::Origin(start)), amount) if *start > 0 && amount > 0 => {
            for (header, height) in headers.iter().zip(*start..*start + amount as u64) {
                if header.height().value() != height {
                    return Err(HeaderExError::InvalidResponse);
                }
            }
        }

        // Check if header has the requested hash
        (Some(Data::Hash(hash)), 1) => {
            if headers[0].hash().as_bytes() != hash {
                return Err(HeaderExError::InvalidResponse);
            }
        }

        _ => return Err(HeaderExError::InvalidResponse),
    }

    Ok(headers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventChannel;
    use crate::p2p::header_ex::utils::ExtendedHeaderExt;
    use celestia_proto::p2p::pb::StatusCode;
    use celestia_types::consts::HASH_SIZE;
    use celestia_types::hash::Hash;
    use celestia_types::test_utils::{ExtendedHeaderGenerator, invalidate, unverify};
    use libp2p::swarm::ConnectionId;
    use lumina_utils::test_utils::async_test;
    use lumina_utils::time::sleep;
    use std::collections::VecDeque;
    use std::future::poll_fn;
    use std::io;
    use std::pin::pin;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;
    use tokio::select;

    #[async_test]
    async fn request_height() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(5, 1), tx);

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let expected_header = generator.next();
        let expected = expected_header.to_header_response();

        mock_req.send_n_responses(1, vec![expected]);

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    #[async_test]
    async fn request_hash() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let expected_header = generator.next();
        let expected = expected_header.to_header_response();

        handler.on_send_request(HeaderRequest::with_hash(expected_header.hash()), tx);

        mock_req.send_n_responses(1, vec![expected]);

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    #[async_test]
    async fn request_with_no_peers() {
        let mut peer_tracker = peer_tracker_with_n_peers(0);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, mut rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(5, 1), tx);

        poll_client_for(
            &mut handler,
            &mut mock_req,
            &peer_tracker,
            Duration::from_millis(20),
        )
        .await;
        // We don't have any available peers, so we shouldn't get any response.
        rx.try_recv().unwrap_err();

        // A peer is now connected
        let peer_id = PeerId::random();
        peer_tracker.add_connection(&peer_id, ConnectionId::new_unchecked(1));

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let expected_header = generator.next();
        let expected = expected_header.to_header_response();

        mock_req.send_n_responses(1, vec![expected]);

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    #[async_test]
    async fn request_range() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(5, 3), tx);

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let expected_headers = generator.next_many(3);
        let expected = expected_headers
            .iter()
            .map(|header| header.to_header_response())
            .collect::<Vec<_>>();

        mock_req.send_n_responses(1, expected);

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result, expected_headers);
    }

    #[async_test]
    async fn request_range_responds_with_unsorted_headers() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(5, 3), tx);

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = generator.next();
        let header6 = generator.next();
        let header7 = generator.next();

        let response = vec![
            header7.to_header_response(),
            header5.to_header_response(),
            header6.to_header_response(),
        ];
        let expected_headers = vec![header5, header6, header7];

        mock_req.send_n_responses(1, response);

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result, expected_headers);
    }

    #[async_test]
    async fn request_range_responds_with_invalid_headaer_in_the_middle() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(5, 5), tx);

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let mut headers = generator.next_many(5);

        invalidate(&mut headers[2]);
        let expected_headers = &headers[..2];

        let responses = headers
            .iter()
            .map(|header| header.to_header_response())
            .collect::<Vec<_>>();

        mock_req.send_n_responses(1, responses);

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result, expected_headers);
    }

    #[async_test]
    async fn request_range_responds_with_not_found() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(5, 2), tx);

        let response = HeaderResponse {
            body: Vec::new(),
            status_code: StatusCode::NotFound.into(),
        };

        for _ in 0..MAX_TRIES {
            mock_req.send_n_responses(1, vec![response.clone()]);

            poll_client_for(
                &mut handler,
                &mut mock_req,
                &peer_tracker,
                Duration::from_millis(20),
            )
            .await;
        }

        assert!(matches!(
            rx.await.unwrap(),
            Err(P2pError::HeaderEx(HeaderExError::HeaderNotFound))
        ));
    }

    #[async_test]
    async fn respond_with_another_height() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(5, 1), tx);

        let mut generator = ExtendedHeaderGenerator::new_from_height(4);
        let header4 = generator.next();

        for _ in 0..MAX_TRIES {
            mock_req.send_n_responses(1, vec![header4.to_header_response()]);

            poll_client_for(
                &mut handler,
                &mut mock_req,
                &peer_tracker,
                Duration::from_millis(20),
            )
            .await;
        }

        assert!(matches!(
            rx.await.unwrap(),
            Err(P2pError::HeaderEx(HeaderExError::InvalidResponse))
        ));
    }

    #[async_test]
    async fn respond_with_bad_range() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(5, 3), tx);

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = generator.next();
        let _header6 = generator.next();
        let header7 = generator.next();

        for _ in 0..MAX_TRIES {
            mock_req.send_n_responses(
                1,
                vec![
                    header5.to_header_response(),
                    header7.to_header_response(),
                    header7.to_header_response(),
                ],
            );

            poll_client_for(
                &mut handler,
                &mut mock_req,
                &peer_tracker,
                Duration::from_millis(20),
            )
            .await;
        }

        assert!(matches!(
            rx.await.unwrap(),
            Err(P2pError::HeaderEx(HeaderExError::InvalidResponse))
        ));
    }

    #[async_test]
    async fn respond_with_bad_hash() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_hash(Hash::Sha256(rand::random())), tx);

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = generator.next();

        for _ in 0..MAX_TRIES {
            mock_req.send_n_responses(1, vec![header5.to_header_response()]);

            poll_client_for(
                &mut handler,
                &mut mock_req,
                &peer_tracker,
                Duration::from_millis(20),
            )
            .await;
        }

        assert!(matches!(
            rx.await.unwrap(),
            Err(P2pError::HeaderEx(HeaderExError::InvalidResponse))
        ));
    }

    #[async_test]
    async fn request_unavailable_heigh() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(5, 1), tx);

        let response = HeaderResponse {
            body: Vec::new(),
            status_code: StatusCode::NotFound.into(),
        };

        for _ in 0..MAX_TRIES {
            mock_req.send_n_responses(1, vec![response.clone()]);

            poll_client_for(
                &mut handler,
                &mut mock_req,
                &peer_tracker,
                Duration::from_millis(20),
            )
            .await;
        }

        assert!(matches!(
            rx.await.unwrap(),
            Err(P2pError::HeaderEx(HeaderExError::HeaderNotFound))
        ));
    }

    #[async_test]
    async fn respond_with_invalid_status_code() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(5, 1), tx);

        let response = HeaderResponse {
            body: Vec::new(),
            status_code: StatusCode::Invalid.into(),
        };

        for _ in 0..MAX_TRIES {
            mock_req.send_n_responses(1, vec![response.clone()]);

            poll_client_for(
                &mut handler,
                &mut mock_req,
                &peer_tracker,
                Duration::from_millis(20),
            )
            .await;
        }

        assert!(matches!(
            rx.await.unwrap(),
            Err(P2pError::HeaderEx(HeaderExError::InvalidResponse))
        ));
    }

    #[async_test]
    async fn respond_with_unknown_status_code() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(5, 1), tx);

        let response = HeaderResponse {
            body: Vec::new(),
            status_code: 1234,
        };

        for _ in 0..MAX_TRIES {
            mock_req.send_n_responses(1, vec![response.clone()]);

            poll_client_for(
                &mut handler,
                &mut mock_req,
                &peer_tracker,
                Duration::from_millis(20),
            )
            .await;
        }

        assert!(matches!(
            rx.await.unwrap(),
            Err(P2pError::HeaderEx(HeaderExError::InvalidResponse))
        ));
    }

    #[async_test]
    async fn request_range_responds_with_smaller_one() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(5, 2), tx);

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = generator.next();

        mock_req.send_n_responses(1, vec![header5.to_header_response()]);

        let headers = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(headers, vec![header5]);
    }

    #[async_test]
    async fn request_range_responds_with_bigger_one() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(5, 2), tx);

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let headers = generator.next_many(3);
        let response = headers
            .iter()
            .map(|header| header.to_header_response())
            .collect::<Vec<_>>();

        for _ in 0..MAX_TRIES {
            mock_req.send_n_responses(1, response.clone());

            poll_client_for(
                &mut handler,
                &mut mock_req,
                &peer_tracker,
                Duration::from_millis(20),
            )
            .await;
        }

        assert!(matches!(
            rx.await.unwrap(),
            Err(P2pError::HeaderEx(HeaderExError::InvalidResponse))
        ));
    }

    #[async_test]
    async fn respond_with_invalid_header() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(5, 1), tx);

        // HeaderEx client must return a validated header.
        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let mut invalid_header5 = generator.next();
        invalidate(&mut invalid_header5);

        for _ in 0..MAX_TRIES {
            mock_req.send_n_responses(1, vec![invalid_header5.to_header_response()]);

            poll_client_for(
                &mut handler,
                &mut mock_req,
                &peer_tracker,
                Duration::from_millis(20),
            )
            .await;
        }

        assert!(matches!(
            rx.await.unwrap(),
            Err(P2pError::HeaderEx(HeaderExError::InvalidResponse))
        ));
    }

    #[async_test]
    async fn respond_with_allowed_bad_header() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(5, 2), tx);

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);

        // HeaderEx client must not verify the headers, this is done only
        // in `get_verified_headers_range` which is used later on in `Syncer`.
        let mut expected_headers = generator.next_many(2);
        unverify(&mut expected_headers[1]);

        let expected = expected_headers
            .iter()
            .map(|header| header.to_header_response())
            .collect::<Vec<_>>();

        mock_req.send_n_responses(1, expected);

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result, expected_headers);
    }

    #[async_test]
    async fn request_height_then_stop() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(5, 1), tx);

        // Trigger stop
        handler.on_stop();

        assert!(matches!(
            poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx).await,
            Err(P2pError::HeaderEx(HeaderExError::RequestCancelled))
        ));

        // This is special test were we do not response to the request.
        // We avoid panicking on `MockReq` drop by clearing it.
        mock_req.clear_pending_requests();
    }

    #[async_test]
    async fn invalid_requests() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        // Zero amount
        let (tx, rx) = oneshot::channel();
        handler.on_send_request(HeaderRequest::with_origin(5, 0), tx);
        assert!(matches!(
            poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx).await,
            Err(P2pError::HeaderEx(HeaderExError::InvalidRequest))
        ));

        // Head with zero amount
        let (tx, rx) = oneshot::channel();
        handler.on_send_request(HeaderRequest::with_origin(0, 0), tx);
        assert!(matches!(
            poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx).await,
            Err(P2pError::HeaderEx(HeaderExError::InvalidRequest))
        ));

        // Head with more than one amount
        let (tx, rx) = oneshot::channel();
        handler.on_send_request(HeaderRequest::with_origin(0, 2), tx);
        assert!(matches!(
            poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx).await,
            Err(P2pError::HeaderEx(HeaderExError::InvalidRequest))
        ));

        // Invalid hash
        let (tx, rx) = oneshot::channel();
        handler.on_send_request(
            HeaderRequest {
                data: Some(Data::Hash(b"12".to_vec())),
                amount: 1,
            },
            tx,
        );
        assert!(matches!(
            poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx).await,
            Err(P2pError::HeaderEx(HeaderExError::InvalidRequest))
        ));

        // Valid hash with more than one amount
        let (tx, rx) = oneshot::channel();
        handler.on_send_request(
            HeaderRequest {
                data: Some(Data::Hash([0xff; HASH_SIZE].to_vec())),
                amount: 2,
            },
            tx,
        );
        assert!(matches!(
            poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx).await,
            Err(P2pError::HeaderEx(HeaderExError::InvalidRequest))
        ));

        // No data
        let (tx, rx) = oneshot::channel();
        handler.on_send_request(
            HeaderRequest {
                data: None,
                amount: 2,
            },
            tx,
        );
        assert!(matches!(
            poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx).await,
            Err(P2pError::HeaderEx(HeaderExError::InvalidRequest))
        ));
    }

    /// Expects the highest height that was reported by at least 2 peers
    #[async_test]
    async fn head_best() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(0, 1), tx);

        let mut generator = ExtendedHeaderGenerator::new_from_height(3);
        let header3 = generator.next();
        let header4 = generator.next();
        let header5 = generator.next();
        let header6 = generator.next();
        let header7 = generator.next();

        // this header also has height = 5 but has different hash
        let another_header5 = generator.another_of(&header5);

        let expected_header = header5;
        let expected = expected_header.to_header_response();

        mock_req.send_n_responses(1, vec![header3.to_header_response()]);
        mock_req.send_n_responses(2, vec![header4.to_header_response()]);
        mock_req.send_n_responses(1, vec![another_header5.to_header_response()]);
        mock_req.send_n_responses(2, vec![expected]);
        mock_req.send_n_responses(1, vec![header6.to_header_response()]);
        mock_req.send_n_responses(1, vec![header7.to_header_response()]);
        mock_req.send_n_failures(1, OutboundFailure::Timeout);
        mock_req.send_n_failures(1, OutboundFailure::ConnectionClosed);

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    /// Expects the highest height that was reported by at least 2 peers
    #[async_test]
    async fn head_highest_peers() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(0, 1), tx);

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let expected_header = generator.next();
        let expected = expected_header.to_header_response();

        // all headers have height = 5 but different hash
        mock_req.send_n_responses(
            1,
            vec![generator.another_of(&expected_header).to_header_response()],
        );
        mock_req.send_n_responses(
            2,
            vec![generator.another_of(&expected_header).to_header_response()],
        );
        mock_req.send_n_responses(
            1,
            vec![generator.another_of(&expected_header).to_header_response()],
        );
        mock_req.send_n_responses(4, vec![expected]);
        mock_req.send_n_responses(
            2,
            vec![generator.another_of(&expected_header).to_header_response()],
        );

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    /// Expects the highest height
    #[async_test]
    async fn head_highest_height() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(0, 1), tx);

        let mut generator = ExtendedHeaderGenerator::new();
        let mut headers = generator.next_many(10);
        let expected_header = headers.remove(9);
        let expected = expected_header.to_header_response();

        mock_req.send_n_responses(1, vec![expected]);

        for header in headers {
            mock_req.send_n_responses(1, vec![header.to_header_response()]);
        }

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    #[async_test]
    async fn head_request_responds_with_multiple_headers() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(0, 1), tx);

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = generator.next();
        let header6 = generator.next();
        let header7 = generator.next();

        // this header also has height = 5 but has different hash
        let another_header5 = generator.another_of(&header5);

        let expected_header = header5;
        let expected = expected_header.to_header_response();

        mock_req.send_n_responses(1, vec![another_header5.to_header_response()]);
        mock_req.send_n_responses(2, vec![expected]);
        mock_req.send_n_responses(
            7,
            vec![header6.to_header_response(), header7.to_header_response()],
        );

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    #[async_test]
    async fn head_request_responds_with_invalid_headers() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(0, 1), tx);

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = generator.next();

        let mut invalid_header5 = generator.another_of(&header5);
        invalidate(&mut invalid_header5);

        let expected_header = header5;
        let expected = expected_header.to_header_response();

        mock_req.send_n_responses(9, vec![invalid_header5.to_header_response()]);
        mock_req.send_n_responses(1, vec![expected]);

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    #[async_test]
    async fn head_request_responds_only_with_invalid_headers() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, mut rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(0, 1), tx);

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = generator.next();
        let mut invalid_header5 = header5.clone();
        invalidate(&mut invalid_header5);

        // Head request retry indefinitely
        for _ in 0..5 {
            mock_req.send_n_responses(10, vec![invalid_header5.to_header_response()]);

            poll_client_for(
                &mut handler,
                &mut mock_req,
                &peer_tracker,
                Duration::from_millis(20),
            )
            .await;

            rx.try_recv().unwrap_err();
        }

        // Now reply with a valid header
        mock_req.send_n_responses(10, vec![header5.to_header_response()]);

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], header5);
    }

    #[async_test]
    async fn head_request_responds_with_only_failures() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, mut rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(0, 1), tx);

        // Head request retry indefinitely
        for _ in 0..5 {
            mock_req.send_n_failures(5, OutboundFailure::Timeout);
            mock_req.send_n_failures(5, OutboundFailure::ConnectionClosed);

            poll_client_for(
                &mut handler,
                &mut mock_req,
                &peer_tracker,
                Duration::from_millis(20),
            )
            .await;

            rx.try_recv().unwrap_err();
        }

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = generator.next();

        // Now reply with a valid header
        mock_req.send_n_responses(10, vec![header5.to_header_response()]);

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], header5);
    }

    #[async_test]
    async fn head_request_with_one_peer() {
        let peer_tracker = peer_tracker_with_n_peers(1);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(0, 1), tx);

        let mut generator = ExtendedHeaderGenerator::new_from_height(10);
        let expected_header = generator.next();
        let expected = expected_header.to_header_response();

        mock_req.send_n_responses(1, vec![expected]);

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    #[async_test]
    async fn head_request_with_no_peers() {
        let mut peer_tracker = peer_tracker_with_n_peers(0);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, mut rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(0, 1), tx);

        poll_client_for(
            &mut handler,
            &mut mock_req,
            &peer_tracker,
            Duration::from_millis(20),
        )
        .await;
        // We don't have any available peers, so we shouldn't get any response.
        rx.try_recv().unwrap_err();

        // A peer is now connected
        let peer_id = PeerId::random();
        peer_tracker.set_trusted(&peer_id, true);
        peer_tracker.add_connection(&peer_id, ConnectionId::new_unchecked(1));

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let expected_header = generator.next();
        let expected = expected_header.to_header_response();

        mock_req.send_n_responses(1, vec![expected]);

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    #[async_test]
    async fn head_request_then_stop() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, mut rx) = oneshot::channel();

        handler.on_send_request(HeaderRequest::with_origin(0, 1), tx);

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);

        mock_req.send_n_responses(5, vec![generator.next().to_header_response()]);

        // Poll client and give some time to it to consume some of the responses.
        poll_client_for(
            &mut handler,
            &mut mock_req,
            &peer_tracker,
            Duration::from_millis(20),
        )
        .await;
        // Not all requests were answered, so we shouldn't have any response.
        rx.try_recv().unwrap_err();

        // Trigger stop
        handler.on_stop();

        assert!(matches!(
            poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx).await,
            Err(P2pError::HeaderEx(HeaderExError::RequestCancelled))
        ));

        // This is special test were we do not response to all requests.
        // We avoid panicking on `MockReq` drop by clearing pending ones.
        mock_req.clear_pending_requests();
    }

    #[async_test]
    async fn pending_request() {
        let empty_peer_tracker = peer_tracker_with_n_peers(0);
        let peer_tracker = peer_tracker_with_n_peers(1);

        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        assert!(!handler.has_pending_requests());
        handler.on_send_request(HeaderRequest::with_origin(5, 1), tx);
        assert!(handler.has_pending_requests());

        // Try poll without peers
        let ev = poll_client(&mut handler, &mut mock_req, &empty_peer_tracker).await;
        // `SchedulePendingRequests` is generated when there are pending requests.
        // Since we have no peers, the requests are still pending.
        assert!(matches!(ev, Event::SchedulePendingRequests));
        assert!(handler.has_pending_requests());

        // Poll again with a connected peer
        poll_client_for(
            &mut handler,
            &mut mock_req,
            &peer_tracker,
            Duration::from_millis(10),
        )
        .await;
        // Request is now scheduled (i.e. not pending)
        assert!(!handler.has_pending_requests());

        // Respond with failure
        mock_req.send_n_failures(1, OutboundFailure::ConnectionClosed);
        poll_client_for(
            &mut handler,
            &mut mock_req,
            &peer_tracker,
            Duration::from_millis(10),
        )
        .await;
        // Request is back to pending
        assert!(handler.has_pending_requests());

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let expected_header = generator.next();
        let expected = expected_header.to_header_response();

        // Schedule response and poll again
        mock_req.send_n_responses(1, vec![expected]);

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
        assert!(!handler.has_pending_requests());
    }

    #[async_test]
    async fn pending_head_request() {
        let empty_peer_tracker = peer_tracker_with_n_peers(0);
        let peer_tracker = peer_tracker_with_n_peers(1);

        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        assert!(!handler.has_pending_requests());
        handler.on_send_request(HeaderRequest::head_request(), tx);
        assert!(handler.has_pending_requests());

        // Try poll without peers
        let ev = poll_client(&mut handler, &mut mock_req, &empty_peer_tracker).await;
        assert!(matches!(ev, Event::NeedTrustedPeers));
        assert!(handler.has_pending_requests());

        // Poll again with a trusted peer
        poll_client_for(
            &mut handler,
            &mut mock_req,
            &peer_tracker,
            Duration::from_millis(10),
        )
        .await;
        // Request is now scheduled (i.e. not pending)
        assert!(!handler.has_pending_requests());

        // Respond with failure
        mock_req.send_n_failures(1, OutboundFailure::ConnectionClosed);
        poll_client_for(
            &mut handler,
            &mut mock_req,
            &peer_tracker,
            Duration::from_millis(10),
        )
        .await;
        // Request is back to pending
        assert!(handler.has_pending_requests());

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let expected_header = generator.next();
        let expected = expected_header.to_header_response();

        // Schedule response and poll again
        mock_req.send_n_responses(1, vec![expected]);

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
        assert!(!handler.has_pending_requests());
    }

    #[async_test]
    async fn pending_archival_request() {
        let mut peer_tracker = peer_tracker_with_n_peers(0);

        let peer_id = PeerId::random();
        peer_tracker.add_connection(&peer_id, ConnectionId::new_unchecked(1));

        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new();

        let (tx, rx) = oneshot::channel();

        assert!(!handler.has_pending_requests());
        handler.on_send_request(HeaderRequest::with_origin(5, 1), tx);

        for _ in 0..MAX_TRIES - 1 {
            assert!(handler.has_pending_requests());

            mock_req.send_n_failures(1, OutboundFailure::ConnectionClosed);

            poll_client_for(
                &mut handler,
                &mut mock_req,
                &peer_tracker,
                Duration::from_millis(20),
            )
            .await;
        }

        assert!(handler.has_pending_requests());
        let ev = poll_client(&mut handler, &mut mock_req, &peer_tracker).await;
        assert!(matches!(ev, Event::NeedArchivalPeers));
        assert!(handler.has_pending_requests());

        // Mark peer as archival
        peer_tracker.mark_as_archival(&peer_id);

        // Poll again
        poll_client_for(
            &mut handler,
            &mut mock_req,
            &peer_tracker,
            Duration::from_millis(10),
        )
        .await;
        // Request is now scheduled (i.e. not pending)
        assert!(!handler.has_pending_requests());

        let mut generator = ExtendedHeaderGenerator::new_from_height(5);
        let expected_header = generator.next();
        let expected = expected_header.to_header_response();

        // Schedule response and poll again
        mock_req.send_n_responses(1, vec![expected]);

        let result = poll_client_and_receiver(&mut handler, &mut mock_req, &peer_tracker, rx)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
        assert!(!handler.has_pending_requests());
    }

    #[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
    struct MockReqId(u64);

    impl MockReqId {
        fn new() -> MockReqId {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            MockReqId(id)
        }
    }

    struct MockReq {
        reqs: VecDeque<MockReqInfo>,
        pending_resp: VecDeque<MockResp>,
    }

    struct MockReqInfo {
        id: MockReqId,
        peer: PeerId,
    }

    enum MockResp {
        Response(usize, Vec<HeaderResponse>),
        Failure(usize, OutboundFailure),
    }

    impl RequestSender for MockReq {
        type RequestId = MockReqId;

        fn send_request(&mut self, peer: &PeerId, _request: HeaderRequest) -> Self::RequestId {
            let id = MockReqId::new();
            self.reqs.push_back(MockReqInfo { id, peer: *peer });
            id
        }
    }

    impl MockReq {
        fn new() -> Self {
            MockReq {
                reqs: VecDeque::new(),
                pending_resp: VecDeque::new(),
            }
        }

        fn send_n_responses(&mut self, n: usize, responses: Vec<HeaderResponse>) {
            self.pending_resp
                .push_back(MockResp::Response(n, responses));
        }

        fn send_n_failures(&mut self, n: usize, error: OutboundFailure) {
            self.pending_resp.push_back(MockResp::Failure(n, error));
        }

        fn clear_pending_requests(&mut self) {
            self.reqs.clear();
        }

        fn schedule_pending_responses(&mut self, handler: &mut HeaderExClientHandler<Self>) {
            for resp in self.pending_resp.drain(..) {
                match resp {
                    MockResp::Response(n, headers) => {
                        for req in self.reqs.drain(..n) {
                            handler.on_response_received(req.peer, req.id, headers.clone());
                        }
                    }
                    MockResp::Failure(n, error) => {
                        for req in self.reqs.drain(..n) {
                            // `OutboundFailure` does not implement `Clone`
                            let error = match error {
                                OutboundFailure::DialFailure => OutboundFailure::DialFailure,
                                OutboundFailure::Timeout => OutboundFailure::Timeout,
                                OutboundFailure::ConnectionClosed => {
                                    OutboundFailure::ConnectionClosed
                                }
                                OutboundFailure::UnsupportedProtocols => {
                                    OutboundFailure::UnsupportedProtocols
                                }
                                OutboundFailure::Io(ref e) => {
                                    OutboundFailure::Io(io::Error::new(e.kind(), ""))
                                }
                            };
                            handler.on_failure(req.peer, req.id, error);
                        }
                    }
                }
            }
        }
    }

    impl Drop for MockReq {
        fn drop(&mut self) {
            assert!(self.reqs.is_empty(), "Not all requests handled");
        }
    }

    fn peer_tracker_with_n_peers(amount: usize) -> PeerTracker {
        let event_channel = EventChannel::new();
        let mut peers = PeerTracker::new(event_channel.publisher());

        for i in 0..amount {
            let peer_id = PeerId::random();
            peers.set_trusted(&peer_id, true);
            peers.add_connection(&peer_id, ConnectionId::new_unchecked(i));

            // After some retries, `HeaderExClientHandler` send the request
            // to an archival node. We make sure we have at least one.
            if i == 0 {
                peers.mark_as_archival(&peer_id);
            }
        }

        peers
    }

    async fn poll_client(
        client: &mut HeaderExClientHandler<MockReq>,
        mock_req: &mut MockReq,
        peer_tracker: &PeerTracker,
    ) -> Event {
        client.schedule_pending_requests(mock_req, peer_tracker);
        mock_req.schedule_pending_responses(client);
        poll_fn(|cx| client.poll(cx)).await
    }

    /// Keep polling `client` until answer is received.
    async fn poll_client_and_receiver(
        client: &mut HeaderExClientHandler<MockReq>,
        mock_req: &mut MockReq,
        peer_tracker: &PeerTracker,
        mut rx: oneshot::Receiver<Result<Vec<ExtendedHeader>, P2pError>>,
    ) -> Result<Vec<ExtendedHeader>, P2pError> {
        loop {
            select! {
                _ = poll_client(client, mock_req, peer_tracker) => {}
                res = &mut rx => return res.unwrap(),
            }
        }
    }

    /// Keep polling `client` until specified duration is reached.
    async fn poll_client_for(
        client: &mut HeaderExClientHandler<MockReq>,
        mock_req: &mut MockReq,
        peer_tracker: &PeerTracker,
        dur: Duration,
    ) {
        let mut sleep = pin!(sleep(dur));

        loop {
            select! {
                _ = poll_client(client, mock_req, peer_tracker) => {}
                _ = &mut sleep => return,
            }
        }
    }
}
