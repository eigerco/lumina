use std::collections::HashMap;
use std::sync::Arc;

use celestia_proto::p2p::pb::{HeaderRequest, HeaderResponse, StatusCode};
use celestia_types::ExtendedHeader;
use libp2p::request_response::{OutboundFailure, RequestId};
use tendermint_proto::Protobuf;
use tracing::{instrument, trace};

use crate::exchange::utils::HeaderRequestExt;
use crate::exchange::{ExchangeError, ReqRespBehaviour};
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

        // TODO: if case of head request, ask multiple peers and return best answer

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

        trace!("Request initiated");
    }

    #[instrument(level = "trace", skip(self, responses), fields(responses.len = responses.len()))]
    pub(super) fn on_response_received(
        &mut self,
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
    pub(super) fn on_failure(&mut self, request_id: RequestId, error: OutboundFailure) {
        trace!("Outbound failure");

        if let Some(req_info) = self.reqs.remove(&request_id) {
            req_info
                .respond_to
                .maybe_send_err(ExchangeError::OutboundFailure(error));
        }
    }
}
