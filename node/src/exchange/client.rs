use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;

use celestia_proto::p2p::pb::{HeaderRequest, HeaderResponse, StatusCode};
use celestia_types::ExtendedHeader;
use futures::future::join_all;
use libp2p::request_response::{OutboundFailure, RequestId};
use libp2p::PeerId;
use tendermint_proto::Protobuf;
use tokio::sync::oneshot;
use tracing::{instrument, trace};

use crate::exchange::utils::HeaderRequestExt;
use crate::exchange::{ExchangeError, ReqRespBehaviour};
use crate::executor::spawn;
use crate::p2p::P2pError;
use crate::peer_tracker::PeerTracker;
use crate::utils::{OneshotResultSender, OneshotResultSenderExt};

pub(super) struct ExchangeClientHandler {
    reqs: HashMap<RequestId, ReqInfo>,
    peer_tracker: Arc<PeerTracker>,
}

struct ReqInfo {
    amount: usize,
    respond_to: OneshotResultSender<Vec<ExtendedHeader>, P2pError>,
}

impl ExchangeClientHandler {
    pub(super) fn new(peer_tracker: Arc<PeerTracker>) -> Self {
        ExchangeClientHandler {
            reqs: HashMap::new(),
            peer_tracker,
        }
    }

    #[instrument(level = "trace", skip(self, req_resp, respond_to))]
    pub(super) fn on_send_request(
        &mut self,
        req_resp: &mut ReqRespBehaviour,
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
        req_resp: &mut ReqRespBehaviour,
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
        req_resp: &mut ReqRespBehaviour,
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
        request_id: RequestId,
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
            let res = match response.status_code() {
                StatusCode::Invalid => Err(ExchangeError::InvalidResponse),
                StatusCode::NotFound => Err(ExchangeError::HeaderNotFound),
                StatusCode::Ok => ExtendedHeader::decode(&response.body[..])
                    .map_err(|_| ExchangeError::InvalidResponse),
            };

            match res {
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
        request_id: RequestId,
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
