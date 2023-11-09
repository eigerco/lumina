use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::task::{Context, Poll};

use celestia_proto::p2p::pb::header_request::Data;
use celestia_proto::p2p::pb::{HeaderRequest, HeaderResponse};
use celestia_types::ExtendedHeader;
use futures::future::join_all;
use libp2p::request_response::{OutboundFailure, OutboundRequestId};
use libp2p::PeerId;
use tokio::sync::oneshot;
use tracing::{debug, instrument, trace};

use crate::executor::{spawn, yield_now};
use crate::header_ex::utils::{HeaderRequestExt, HeaderResponseExt};
use crate::header_ex::{HeaderExError, ReqRespBehaviour};
use crate::p2p::P2pError;
use crate::peer_tracker::PeerTracker;
use crate::utils::{OneshotResultSender, OneshotResultSenderExt, VALIDATIONS_PER_YIELD};

const MAX_PEERS: usize = 10;

pub(super) struct HeaderExClientHandler<S = ReqRespBehaviour>
where
    S: RequestSender,
{
    reqs: HashMap<S::RequestId, State>,
    peer_tracker: Arc<PeerTracker>,
}

struct State {
    request: HeaderRequest,
    respond_to: OneshotResultSender<Vec<ExtendedHeader>, P2pError>,
}

pub(super) trait RequestSender {
    type RequestId: Clone + Copy + Hash + Eq + Debug;

    fn send_request(&mut self, peer: &PeerId, request: HeaderRequest) -> Self::RequestId;
}

impl RequestSender for ReqRespBehaviour {
    type RequestId = OutboundRequestId;

    fn send_request(&mut self, peer: &PeerId, request: HeaderRequest) -> OutboundRequestId {
        self.send_request(peer, request)
    }
}

impl<S> HeaderExClientHandler<S>
where
    S: RequestSender,
{
    pub(super) fn new(peer_tracker: Arc<PeerTracker>) -> Self {
        HeaderExClientHandler {
            reqs: HashMap::new(),
            peer_tracker,
        }
    }

    #[instrument(level = "trace", skip(self, sender, respond_to))]
    pub(super) fn on_send_request(
        &mut self,
        sender: &mut S,
        request: HeaderRequest,
        respond_to: OneshotResultSender<Vec<ExtendedHeader>, P2pError>,
    ) {
        if !request.is_valid() {
            respond_to.maybe_send_err(HeaderExError::InvalidRequest);
            return;
        }

        if request.is_head_request() {
            self.send_head_request(sender, request, respond_to);
        } else {
            self.send_request(sender, request, respond_to);
        }

        trace!("Request initiated");
    }

    fn send_request(
        &mut self,
        sender: &mut S,
        request: HeaderRequest,
        respond_to: OneshotResultSender<Vec<ExtendedHeader>, P2pError>,
    ) {
        // Validate amount
        if usize::try_from(request.amount).is_err() {
            respond_to.maybe_send_err(HeaderExError::InvalidRequest);
            return;
        };

        let Some(peer) = self.peer_tracker.best_peer() else {
            respond_to.maybe_send_err(P2pError::NoConnectedPeers);
            return;
        };

        let req_id = sender.send_request(&peer, request.clone());
        let state = State {
            request,
            respond_to,
        };

        self.reqs.insert(req_id, state);
    }

    fn send_head_request(
        &mut self,
        sender: &mut S,
        request: HeaderRequest,
        respond_to: OneshotResultSender<Vec<ExtendedHeader>, P2pError>,
    ) {
        const MIN_HEAD_RESPONSES: usize = 2;

        // For now HEAD is requested from trusted peers only!
        let peers = self.peer_tracker.trusted_n_peers(MAX_PEERS);

        if peers.is_empty() {
            respond_to.maybe_send_err(P2pError::NoConnectedPeers);
            return;
        }

        let mut rxs = Vec::with_capacity(peers.len());

        for peer in peers {
            let (tx, rx) = oneshot::channel();

            let req_id = sender.send_request(&peer, request.clone());
            let state = State {
                request: request.clone(),
                respond_to: tx,
            };

            self.reqs.insert(req_id, state);
            rxs.push(rx);
        }

        // Choose the best HEAD.
        //
        // Algorithm: https://github.com/celestiaorg/go-header/blob/e50090545cc7e049d2f965d2b5c773eaa4a2c0b2/p2p/exchange.go#L357-L381
        spawn(async move {
            let mut resps: Vec<_> = join_all(rxs)
                .await
                .into_iter()
                // In case of HEAD all responses have only 1 header.
                // This was already enforced by `decode_and_verify_responses`.
                .filter_map(|v| v.ok()?.ok()?.into_iter().next())
                .collect();
            let mut counter: HashMap<_, usize> = HashMap::new();

            // In case of no responses, Celestia handles it as NotFound
            if resps.is_empty() {
                respond_to.maybe_send_err(HeaderExError::HeaderNotFound);
                return;
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
                    respond_to.maybe_send_ok(vec![resp.to_owned()]);
                    return;
                }
            }

            // Otherwise return the header with the maximum height
            let resp = resps.into_iter().next().expect("no reposnes");
            respond_to.maybe_send_ok(vec![resp]);
        });
    }

    #[instrument(level = "trace", skip(self, responses), fields(responses.len = responses.len()))]
    pub(super) fn on_response_received(
        &mut self,
        peer: PeerId,
        request_id: S::RequestId,
        responses: Vec<HeaderResponse>,
    ) {
        let Some(state) = self.reqs.remove(&request_id) else {
            return;
        };

        trace!(
            "Response received. Expected amount = {}",
            state.request.amount
        );

        spawn(async move {
            match decode_and_verify_responses(&state.request, &responses).await {
                Ok(headers) => {
                    // TODO: Increase peer score
                    state.respond_to.maybe_send_ok(headers);
                }
                Err(e) => {
                    // TODO: Decrease peer score
                    state.respond_to.maybe_send_err(e);
                }
            }
        });
    }

    #[instrument(level = "debug", skip(self))]
    pub(super) fn on_failure(
        &mut self,
        peer: PeerId,
        request_id: S::RequestId,
        error: OutboundFailure,
    ) {
        debug!("Outbound failure");

        if let Some(state) = self.reqs.remove(&request_id) {
            state
                .respond_to
                .maybe_send_err(HeaderExError::OutboundFailure(error));
        }
    }

    pub(super) fn poll(&mut self, _cx: &mut Context) -> Poll<()> {
        Poll::Pending
    }
}

async fn decode_and_verify_responses(
    request: &HeaderRequest,
    responses: &[HeaderResponse],
) -> Result<Vec<ExtendedHeader>, HeaderExError> {
    if responses.is_empty() {
        return Err(HeaderExError::InvalidResponse);
    }

    let amount = usize::try_from(request.amount).expect("validated in send_request");

    // Server shouldn't respond with more headers
    if responses.len() > amount {
        return Err(HeaderExError::InvalidResponse);
    }

    let mut headers = Vec::with_capacity(responses.len());

    'outer: for responses in responses.chunks(VALIDATIONS_PER_YIELD) {
        for response in responses {
            // Unmarshal and validate. Propagate error only if nothing
            // was decoded before.
            let header = match response.to_extended_header() {
                Ok(header) => header,
                Err(e) if headers.is_empty() => return Err(e),
                Err(_) => break 'outer,
            };

            trace!("Header: {header:?}");
            headers.push(header);
        }

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
    use crate::header_ex::utils::ExtendedHeaderExt;
    use celestia_proto::p2p::pb::header_request::Data;
    use celestia_proto::p2p::pb::StatusCode;
    use celestia_types::consts::HASH_SIZE;
    use celestia_types::hash::Hash;
    use celestia_types::test_utils::{invalidate, unverify, ExtendedHeaderGenerator};
    use libp2p::swarm::ConnectionId;
    use std::collections::VecDeque;
    use std::io;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[cfg(not(target_arch = "wasm32"))]
    use tokio::test as async_test;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as async_test;

    #[async_test]
    async fn request_height() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(5, 1), tx);

        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let expected_header = gen.next();
        let expected = expected_header.to_header_response();

        mock_req.send_n_responses(&mut handler, 1, vec![expected]);

        let result = rx.await.unwrap().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    #[async_test]
    async fn request_hash() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let expected_header = gen.next();
        let expected = expected_header.to_header_response();

        handler.on_send_request(
            &mut mock_req,
            HeaderRequest::with_hash(expected_header.hash()),
            tx,
        );

        mock_req.send_n_responses(&mut handler, 1, vec![expected]);

        let result = rx.await.unwrap().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    #[async_test]
    async fn request_range() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(5, 3), tx);

        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let expected_headers = gen.next_many(3);
        let expected = expected_headers
            .iter()
            .map(|header| header.to_header_response())
            .collect::<Vec<_>>();

        mock_req.send_n_responses(&mut handler, 1, expected);

        let result = rx.await.unwrap().unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result, expected_headers);
    }

    #[async_test]
    async fn request_range_responds_with_unsorted_headers() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(5, 3), tx);

        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = gen.next();
        let header6 = gen.next();
        let header7 = gen.next();

        let response = vec![
            header7.to_header_response(),
            header5.to_header_response(),
            header6.to_header_response(),
        ];
        let expected_headers = vec![header5, header6, header7];

        mock_req.send_n_responses(&mut handler, 1, response);

        let result = rx.await.unwrap().unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result, expected_headers);
    }

    #[async_test]
    async fn request_range_responds_with_invalid_headaer_in_the_middle() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(5, 5), tx);

        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let mut headers = gen.next_many(5);

        invalidate(&mut headers[2]);
        let expected_headers = &headers[..2];

        let responses = headers
            .iter()
            .map(|header| header.to_header_response())
            .collect::<Vec<_>>();

        mock_req.send_n_responses(&mut handler, 1, responses);

        let result = rx.await.unwrap().unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result, expected_headers);
    }

    #[async_test]
    async fn request_range_responds_with_not_found() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(5, 2), tx);

        let response = HeaderResponse {
            body: Vec::new(),
            status_code: StatusCode::NotFound.into(),
        };

        mock_req.send_n_responses(&mut handler, 1, vec![response]);

        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::HeaderEx(HeaderExError::HeaderNotFound)))
        ));
    }

    #[async_test]
    async fn respond_with_another_height() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(5, 1), tx);

        let mut gen = ExtendedHeaderGenerator::new_from_height(4);
        let header4 = gen.next();

        mock_req.send_n_responses(&mut handler, 1, vec![header4.to_header_response()]);

        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::HeaderEx(HeaderExError::InvalidResponse)))
        ));
    }

    #[async_test]
    async fn respond_with_bad_range() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(5, 3), tx);

        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = gen.next();
        let _header6 = gen.next();
        let header7 = gen.next();

        mock_req.send_n_responses(
            &mut handler,
            1,
            vec![
                header5.to_header_response(),
                header7.to_header_response(),
                header7.to_header_response(),
            ],
        );

        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::HeaderEx(HeaderExError::InvalidResponse)))
        ));
    }

    #[async_test]
    async fn respond_with_bad_hash() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(
            &mut mock_req,
            HeaderRequest::with_hash(Hash::Sha256(rand::random())),
            tx,
        );

        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = gen.next();

        mock_req.send_n_responses(&mut handler, 1, vec![header5.to_header_response()]);

        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::HeaderEx(HeaderExError::InvalidResponse)))
        ));
    }

    #[async_test]
    async fn request_unavailable_heigh() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(5, 1), tx);

        let response = HeaderResponse {
            body: Vec::new(),
            status_code: StatusCode::NotFound.into(),
        };

        mock_req.send_n_responses(&mut handler, 1, vec![response]);

        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::HeaderEx(HeaderExError::HeaderNotFound)))
        ));
    }

    #[async_test]
    async fn respond_with_invalid_status_code() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(5, 1), tx);

        let response = HeaderResponse {
            body: Vec::new(),
            status_code: StatusCode::Invalid.into(),
        };

        mock_req.send_n_responses(&mut handler, 1, vec![response]);

        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::HeaderEx(HeaderExError::InvalidResponse)))
        ));
    }

    #[async_test]
    async fn respond_with_unknown_status_code() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(5, 1), tx);

        let response = HeaderResponse {
            body: Vec::new(),
            status_code: 1234,
        };

        mock_req.send_n_responses(&mut handler, 1, vec![response]);

        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::HeaderEx(HeaderExError::InvalidResponse)))
        ));
    }

    #[async_test]
    #[ignore] // TODO: Enable this test after sessions are implemented
    #[cfg(not(target_arch = "wasm32"))] // wasm_bindgen_test doesn't seem to support #[ignore]
    async fn request_range_responds_with_smaller_one() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(5, 2), tx);

        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = gen.next();

        mock_req.send_n_responses(&mut handler, 1, vec![header5.to_header_response()]);

        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::HeaderEx(HeaderExError::InvalidResponse)))
        ));
    }

    #[async_test]
    async fn request_range_responds_with_bigger_one() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(5, 2), tx);

        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let headers = gen.next_many(3);
        let response = headers
            .iter()
            .map(|header| header.to_header_response())
            .collect::<Vec<_>>();

        mock_req.send_n_responses(&mut handler, 1, response);

        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::HeaderEx(HeaderExError::InvalidResponse)))
        ));
    }

    #[async_test]
    async fn respond_with_invalid_header() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(5, 1), tx);

        // HeaderEx client must return a validated header.
        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let mut invalid_header5 = gen.next();
        invalidate(&mut invalid_header5);

        mock_req.send_n_responses(&mut handler, 1, vec![invalid_header5.to_header_response()]);

        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::HeaderEx(HeaderExError::InvalidResponse)))
        ));
    }

    #[async_test]
    async fn respond_with_allowed_bad_header() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(5, 2), tx);

        let mut gen = ExtendedHeaderGenerator::new_from_height(5);

        // HeaderEx client must not verify the headers, this is done only
        // in `get_verified_headers_range` which is used later on in `Syncer`.
        let mut expected_headers = gen.next_many(2);
        unverify(&mut expected_headers[1]);

        let expected = expected_headers
            .iter()
            .map(|header| header.to_header_response())
            .collect::<Vec<_>>();

        mock_req.send_n_responses(&mut handler, 1, expected);

        let result = rx.await.unwrap().unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result, expected_headers);
    }

    #[async_test]
    async fn invalid_requests() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        // Zero amount
        let (tx, rx) = oneshot::channel();
        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(5, 0), tx);
        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::HeaderEx(HeaderExError::InvalidRequest)))
        ));

        // Head with zero amount
        let (tx, rx) = oneshot::channel();
        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(0, 0), tx);
        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::HeaderEx(HeaderExError::InvalidRequest)))
        ));

        // Head with more than one amount
        let (tx, rx) = oneshot::channel();
        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(0, 2), tx);
        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::HeaderEx(HeaderExError::InvalidRequest)))
        ));

        // Invalid hash
        let (tx, rx) = oneshot::channel();
        handler.on_send_request(
            &mut mock_req,
            HeaderRequest {
                data: Some(Data::Hash(b"12".to_vec())),
                amount: 1,
            },
            tx,
        );
        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::HeaderEx(HeaderExError::InvalidRequest)))
        ));

        // Valid hash with more than one amount
        let (tx, rx) = oneshot::channel();
        handler.on_send_request(
            &mut mock_req,
            HeaderRequest {
                data: Some(Data::Hash([0xff; HASH_SIZE].to_vec())),
                amount: 2,
            },
            tx,
        );
        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::HeaderEx(HeaderExError::InvalidRequest)))
        ));

        // No data
        let (tx, rx) = oneshot::channel();
        handler.on_send_request(
            &mut mock_req,
            HeaderRequest {
                data: None,
                amount: 2,
            },
            tx,
        );
        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::HeaderEx(HeaderExError::InvalidRequest)))
        ));
    }

    /// Expects the highest height that was reported by at least 2 peers
    #[async_test]
    async fn head_best() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(0, 1), tx);

        let mut gen = ExtendedHeaderGenerator::new_from_height(3);
        let header3 = gen.next();
        let header4 = gen.next();
        let header5 = gen.next();
        let header6 = gen.next();
        let header7 = gen.next();

        // this header also has height = 5 but has different hash
        let another_header5 = gen.another_of(&header5);

        let expected_header = header5;
        let expected = expected_header.to_header_response();

        mock_req.send_n_responses(&mut handler, 1, vec![header3.to_header_response()]);
        mock_req.send_n_responses(&mut handler, 2, vec![header4.to_header_response()]);
        mock_req.send_n_responses(&mut handler, 1, vec![another_header5.to_header_response()]);
        mock_req.send_n_responses(&mut handler, 2, vec![expected]);
        mock_req.send_n_responses(&mut handler, 1, vec![header6.to_header_response()]);
        mock_req.send_n_responses(&mut handler, 1, vec![header7.to_header_response()]);
        mock_req.send_n_failures(&mut handler, 1, OutboundFailure::Timeout);
        mock_req.send_n_failures(&mut handler, 1, OutboundFailure::ConnectionClosed);

        let result = rx.await.unwrap().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    /// Expects the highest height that was reported by at least 2 peers
    #[async_test]
    async fn head_highest_peers() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(0, 1), tx);

        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let expected_header = gen.next();
        let expected = expected_header.to_header_response();

        // all headers have height = 5 but different hash
        mock_req.send_n_responses(
            &mut handler,
            1,
            vec![gen.another_of(&expected_header).to_header_response()],
        );
        mock_req.send_n_responses(
            &mut handler,
            2,
            vec![gen.another_of(&expected_header).to_header_response()],
        );
        mock_req.send_n_responses(
            &mut handler,
            1,
            vec![gen.another_of(&expected_header).to_header_response()],
        );
        mock_req.send_n_responses(&mut handler, 4, vec![expected]);
        mock_req.send_n_responses(
            &mut handler,
            2,
            vec![gen.another_of(&expected_header).to_header_response()],
        );

        let result = rx.await.unwrap().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    /// Expects the highest height
    #[async_test]
    async fn head_highest_height() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(0, 1), tx);

        let mut gen = ExtendedHeaderGenerator::new();
        let mut headers = gen.next_many(10);
        let expected_header = headers.remove(9);
        let expected = expected_header.to_header_response();

        mock_req.send_n_responses(&mut handler, 1, vec![expected]);

        for header in headers {
            mock_req.send_n_responses(&mut handler, 1, vec![header.to_header_response()]);
        }

        let result = rx.await.unwrap().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    #[async_test]
    async fn head_request_responds_with_multiple_headers() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(0, 1), tx);

        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = gen.next();
        let header6 = gen.next();
        let header7 = gen.next();

        // this header also has height = 5 but has different hash
        let another_header5 = gen.another_of(&header5);

        let expected_header = header5;
        let expected = expected_header.to_header_response();

        mock_req.send_n_responses(&mut handler, 1, vec![another_header5.to_header_response()]);
        mock_req.send_n_responses(&mut handler, 2, vec![expected]);
        mock_req.send_n_responses(
            &mut handler,
            7,
            vec![header6.to_header_response(), header7.to_header_response()],
        );

        let result = rx.await.unwrap().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    #[async_test]
    async fn head_request_responds_with_invalid_headers() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(0, 1), tx);

        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let header5 = gen.next();

        let mut invalid_header5 = gen.another_of(&header5);
        invalidate(&mut invalid_header5);

        let expected_header = header5;
        let expected = expected_header.to_header_response();

        mock_req.send_n_responses(&mut handler, 9, vec![invalid_header5.to_header_response()]);
        mock_req.send_n_responses(&mut handler, 1, vec![expected]);

        let result = rx.await.unwrap().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    #[async_test]
    async fn head_request_responds_only_with_invalid_headers() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(0, 1), tx);

        let mut gen = ExtendedHeaderGenerator::new_from_height(5);
        let mut invalid_header5 = gen.next();
        invalidate(&mut invalid_header5);

        mock_req.send_n_responses(&mut handler, 10, vec![invalid_header5.to_header_response()]);

        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::HeaderEx(HeaderExError::HeaderNotFound)))
        ));
    }

    #[async_test]
    async fn head_request_responds_with_only_failures() {
        let peer_tracker = peer_tracker_with_n_peers(15);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(0, 1), tx);

        mock_req.send_n_failures(&mut handler, 5, OutboundFailure::Timeout);
        mock_req.send_n_failures(&mut handler, 5, OutboundFailure::ConnectionClosed);

        assert!(matches!(
            rx.await,
            Ok(Err(P2pError::HeaderEx(HeaderExError::HeaderNotFound)))
        ));
    }

    #[async_test]
    async fn head_request_with_one_peer() {
        let peer_tracker = peer_tracker_with_n_peers(1);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(0, 1), tx);

        let mut gen = ExtendedHeaderGenerator::new_from_height(10);
        let expected_header = gen.next();
        let expected = expected_header.to_header_response();

        mock_req.send_n_responses(&mut handler, 1, vec![expected]);

        let result = rx.await.unwrap().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected_header);
    }

    #[async_test]
    async fn head_request_with_no_peers() {
        let peer_tracker = peer_tracker_with_n_peers(0);
        let mut mock_req = MockReq::new();
        let mut handler = HeaderExClientHandler::<MockReq>::new(peer_tracker);

        let (tx, rx) = oneshot::channel();

        handler.on_send_request(&mut mock_req, HeaderRequest::with_origin(0, 1), tx);

        assert!(matches!(rx.await, Ok(Err(P2pError::NoConnectedPeers))));
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
    }

    struct MockReqInfo {
        id: MockReqId,
        peer: PeerId,
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
            }
        }

        fn send_n_responses(
            &mut self,
            handler: &mut HeaderExClientHandler<Self>,
            n: usize,
            responses: Vec<HeaderResponse>,
        ) {
            for req in self.reqs.drain(..n) {
                handler.on_response_received(req.peer, req.id, responses.clone());
            }
        }

        fn send_n_failures(
            &mut self,
            handler: &mut HeaderExClientHandler<Self>,
            n: usize,
            error: OutboundFailure,
        ) {
            for req in self.reqs.drain(..n) {
                // `OutboundFailure` does not implement `Clone`
                let error = match error {
                    OutboundFailure::DialFailure => OutboundFailure::DialFailure,
                    OutboundFailure::Timeout => OutboundFailure::Timeout,
                    OutboundFailure::ConnectionClosed => OutboundFailure::ConnectionClosed,
                    OutboundFailure::UnsupportedProtocols => OutboundFailure::UnsupportedProtocols,
                    OutboundFailure::Io(ref e) => OutboundFailure::Io(io::Error::new(e.kind(), "")),
                };
                handler.on_failure(req.peer, req.id, error);
            }
        }
    }

    impl Drop for MockReq {
        fn drop(&mut self) {
            assert!(self.reqs.is_empty(), "Not all requests handled");
        }
    }

    fn peer_tracker_with_n_peers(amount: usize) -> Arc<PeerTracker> {
        let peers = Arc::new(PeerTracker::new());

        for i in 0..amount {
            let peer = PeerId::random();
            peers.set_trusted(peer, true);
            peers.set_connected(peer, ConnectionId::new_unchecked(i), None);
        }

        peers
    }
}
