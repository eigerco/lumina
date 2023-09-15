use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

use celestia_proto::p2p::pb::{HeaderRequest, HeaderResponse};
use celestia_types::ExtendedHeader;
use futures::future::join_all;
use libp2p::request_response::{OutboundFailure, RequestId};
use libp2p::PeerId;
use tokio::sync::oneshot;
use tracing::{instrument, trace};

use crate::exchange::utils::{HeaderRequestExt, HeaderResponseExt};
use crate::exchange::{ExchangeError, ReqRespBehaviour};
use crate::executor::spawn;
use crate::p2p::P2pError;
use crate::peer_tracker::PeerTracker;
use crate::utils::{OneshotResultSender, OneshotResultSenderExt};

pub(super) struct ExchangeClientHandler<R = ReqRespBehaviour>
where
    R: Req,
{
    reqs: HashMap<R::ReqId, ReqInfo>,
    peer_tracker: Arc<PeerTracker>,
}

struct ReqInfo {
    amount: usize,
    respond_to: OneshotResultSender<Vec<ExtendedHeader>, P2pError>,
}

pub(super) trait Req {
    type ReqId: Hash + Eq + Debug;

    fn send_request(&mut self, peer: &PeerId, request: HeaderRequest) -> Self::ReqId;
}

impl Req for ReqRespBehaviour {
    type ReqId = RequestId;

    fn send_request(&mut self, peer: &PeerId, request: HeaderRequest) -> RequestId {
        self.send_request(peer, request)
    }
}

impl<R> ExchangeClientHandler<R>
where
    R: Req,
{
    pub(super) fn new(peer_tracker: Arc<PeerTracker>) -> Self {
        ExchangeClientHandler {
            reqs: HashMap::new(),
            peer_tracker,
        }
    }

    #[instrument(level = "trace", skip(self, req_resp, respond_to))]
    pub(super) fn on_send_request(
        &mut self,
        req_resp: &mut R,
        request: HeaderRequest,
        respond_to: OneshotResultSender<Vec<ExtendedHeader>, P2pError>,
    ) {
        if !request.is_valid() {
            respond_to.maybe_send_err(ExchangeError::InvalidRequest);
            return;
        }

        if request.is_head_request() {
            self.send_head_request(req_resp, request, respond_to);
        } else {
            self.send_request(req_resp, request, respond_to);
        }

        trace!("Request initiated");
    }

    fn send_request(
        &mut self,
        req_resp: &mut R,
        request: HeaderRequest,
        respond_to: OneshotResultSender<Vec<ExtendedHeader>, P2pError>,
    ) {
        // Convert amount to usize
        let Ok(amount) = usize::try_from(request.amount) else {
            respond_to.maybe_send_err(ExchangeError::InvalidRequest);
            return;
        };

        let Some(peer) = self.peer_tracker.best_peer() else {
            respond_to.maybe_send_err(P2pError::NoPeers);
            return;
        };

        let req_id = req_resp.send_request(&peer, request);
        self.reqs.insert(req_id, ReqInfo { amount, respond_to });
    }

    fn send_head_request(
        &mut self,
        req_resp: &mut R,
        request: HeaderRequest,
        respond_to: OneshotResultSender<Vec<ExtendedHeader>, P2pError>,
    ) {
        const MAX_PEERS: usize = 10;
        const MIN_HEAD_RESPONSES: usize = 2;

        let Some(peers) = self.peer_tracker.best_n_peers(MAX_PEERS) else {
            respond_to.maybe_send_err(P2pError::NoPeers);
            return;
        };

        let mut rxs = Vec::with_capacity(peers.len());

        for peer in peers {
            let (tx, rx) = oneshot::channel();

            let req_id = req_resp.send_request(&peer, request.clone());
            let req_info = ReqInfo {
                amount: 1,
                respond_to: tx,
            };

            self.reqs.insert(req_id, req_info);
            rxs.push(rx);
        }

        // Choose the best HEAD.
        //
        // Algorithm: https://github.com/celestiaorg/go-header/blob/e50090545cc7e049d2f965d2b5c773eaa4a2c0b2/p2p/exchange.go#L357-L381
        spawn(async move {
            let mut resps: Vec<_> = join_all(rxs)
                .await
                .into_iter()
                .filter_map(|v| v.ok()?.ok()?.into_iter().next())
                .collect();
            let mut counter: HashMap<_, usize> = HashMap::new();

            // In case of no responses, Celestia handles it as NotFound
            if resps.is_empty() {
                respond_to.maybe_send_err(ExchangeError::HeaderNotFound);
                return;
            }

            // Sort by height in descending order
            resps.sort_by_key(|resp| Reverse(resp.height()));

            // Count peers per response
            for resp in &resps {
                *counter.entry(resp.hash()).or_default() += 1;
            }

            // Return the header with the maximum height that was received by at least 2 peers
            for resp in &resps {
                if counter[&resp.hash()] >= MIN_HEAD_RESPONSES {
                    respond_to.maybe_send_ok(vec![resp.to_owned()]);
                    return;
                }
            }

            // Otherwise return the header with the maxium height
            respond_to.maybe_send_ok(vec![resps[0].to_owned()]);
        });
    }

    #[instrument(level = "trace", skip(self, responses), fields(responses.len = responses.len()))]
    pub(super) fn on_response_received(
        &mut self,
        peer: PeerId,
        request_id: R::ReqId,
        responses: Vec<HeaderResponse>,
    ) {
        let Some(ReqInfo { amount, respond_to }) = self.reqs.remove(&request_id) else {
            return;
        };

        trace!("Response received. Expected amount = {amount}");

        if responses.len() != amount {
            // TODO: should we define a separate error for this case?
            respond_to.maybe_send_err(ExchangeError::InvalidResponse);
            return;
        }

        let mut headers = Vec::with_capacity(amount);

        for response in responses {
            match response.to_extended_header() {
                Ok(header) => {
                    trace!("Header: {header:?}");
                    headers.push(header);
                }
                Err(e) => {
                    respond_to.maybe_send_err(e);
                    return;
                }
            }
        }

        respond_to.maybe_send_ok(headers);
    }

    #[instrument(level = "trace", skip(self))]
    pub(super) fn on_failure(
        &mut self,
        peer: PeerId,
        request_id: R::ReqId,
        error: OutboundFailure,
    ) {
        trace!("Outbound failure");

        if let Some(req_info) = self.reqs.remove(&request_id) {
            req_info
                .respond_to
                .maybe_send_err(ExchangeError::OutboundFailure(error));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::utils::ExtendedHeaderExt;
    use celestia_types::{DataAvailabilityHeader, Hash, ValidatorSet};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tendermint::block::header::Version;
    use tendermint::block::{Commit, Header, Id};
    use tendermint::hash::AppHash;
    use tendermint::Time;

    /// Expects the highest heigh that was reported by at least 2 peers
    #[tokio::test]
    async fn head_returns_response_with_at_least_two_peers() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = ExchangeClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(0, 1), tx);

        let expected = gen_header_response(5);
        let expected_header = expected.to_extended_header().unwrap();

        mock_req.send_n_responses(&mut handler, 1, vec![gen_header_response(3)]);
        mock_req.send_n_responses(&mut handler, 2, vec![gen_header_response(4)]);
        mock_req.send_n_responses(&mut handler, 1, vec![gen_header_response(5)]);
        mock_req.send_n_responses(&mut handler, 2, vec![expected.clone()]);
        mock_req.send_n_responses(&mut handler, 1, vec![gen_header_response(6)]);
        mock_req.send_n_responses(&mut handler, 1, vec![gen_header_response(7)]);
        mock_req.send_n_failures(&mut handler, 1, OutboundFailure::Timeout);
        mock_req.send_n_failures(&mut handler, 1, OutboundFailure::ConnectionClosed);

        let result = rx.await.unwrap().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    /// Expects the highest height
    #[tokio::test]
    async fn head_returns_response_highest_height() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = ExchangeClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(0, 1), tx);

        let expected = gen_header_response(10);
        let expected_header = expected.to_extended_header().unwrap();

        mock_req.send_n_responses(&mut handler, 1, vec![expected.clone()]);

        for height in 1..10 {
            mock_req.send_n_responses(&mut handler, 1, vec![gen_header_response(height)]);
        }

        let result = rx.await.unwrap().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    #[tokio::test]
    async fn head_handles_only_failures() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = ExchangeClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(0, 1), tx);

        mock_req.send_n_failures(&mut handler, 5, OutboundFailure::Timeout);
        mock_req.send_n_failures(&mut handler, 5, OutboundFailure::ConnectionClosed);

        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::Exchange(ExchangeError::HeaderNotFound)))
        ));
    }

    #[tokio::test]
    async fn head_request_with_one_peer() {
        let peer_tracker = peer_tracker_with_n_peers(1);
        let mut mock_req = MockReq::new();
        let mut handler = ExchangeClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(0, 1), tx);

        let expected = gen_header_response(10);
        let expected_header = expected.to_extended_header().unwrap();

        mock_req.send_n_responses(&mut handler, 1, vec![expected.clone()]);

        let result = rx.await.unwrap().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    #[tokio::test]
    async fn head_request_with_no_peers() {
        let peer_tracker = peer_tracker_with_n_peers(0);
        let mut mock_req = MockReq::new();
        let mut handler = ExchangeClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(0, 1), tx);

        assert!(matches!(rx.await, Ok(Err(P2pError::NoPeers))));
    }

    #[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
    struct RequestId(u64);

    impl RequestId {
        fn new() -> RequestId {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            RequestId(id)
        }
    }

    struct MockReq {
        reqs: VecDeque<MockReqInfo>,
    }

    struct MockReqInfo {
        id: RequestId,
        peer: PeerId,
    }

    impl Req for MockReq {
        type ReqId = RequestId;

        fn send_request(&mut self, peer: &PeerId, _request: HeaderRequest) -> Self::ReqId {
            let id = RequestId::new();
            self.reqs.push_back(MockReqInfo { id, peer: *peer });
            id
        }
    }

    impl MockReq {
        fn new() -> Self {
            MockReq {
                reqs: VecDeque::new(),
            }
        }

        fn send_n_responses(
            &mut self,
            handler: &mut ExchangeClientHandler<Self>,
            n: usize,
            responses: Vec<HeaderResponse>,
        ) {
            for req in self.reqs.drain(..n) {
                handler.on_response_received(req.peer, req.id, responses.clone());
            }
        }

        fn send_n_failures(
            &mut self,
            handler: &mut ExchangeClientHandler<Self>,
            n: usize,
            error: OutboundFailure,
        ) {
            for req in self.reqs.drain(..n) {
                handler.on_failure(req.peer, req.id, error.clone());
            }
        }
    }

    fn gen_header_response(height: u64) -> HeaderResponse {
        ExtendedHeader {
            header: Header {
                version: Version { block: 11, app: 1 },
                chain_id: "private".to_string().try_into().unwrap(),
                height: height.try_into().unwrap(),
                time: Time::now(),
                last_block_id: None,
                last_commit_hash: Hash::default(),
                data_hash: Hash::default(),
                validators_hash: Hash::default(),
                next_validators_hash: Hash::default(),
                consensus_hash: Hash::default(),
                app_hash: AppHash::default(),
                last_results_hash: Hash::default(),
                evidence_hash: Hash::default(),
                proposer_address: tendermint::account::Id::new([0; 20]),
            },
            commit: Commit {
                height: height.try_into().unwrap(),
                block_id: Id {
                    hash: Hash::Sha256(rand::random()),
                    ..Default::default()
                },
                ..Default::default()
            },
            validator_set: ValidatorSet::new(Vec::new(), None),
            dah: DataAvailabilityHeader {
                row_roots: Vec::new(),
                column_roots: Vec::new(),
                hash: [0; 32],
            },
        }
        .to_header_response()
    }

    fn gen_n_peers(n: usize) -> Vec<PeerId> {
        (0..n).map(|_| PeerId::random()).collect()
    }

    fn peer_tracker_with_n_peers(amount: usize) -> Arc<PeerTracker> {
        let peers = Arc::new(PeerTracker::new());
        peers.add_many(gen_n_peers(amount));
        peers
    }
}
