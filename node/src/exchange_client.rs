use std::collections::HashMap;

use celestia_proto::p2p::pb::{HeaderResponse, StatusCode};
use celestia_types::ExtendedHeader;
use libp2p::request_response::{OutboundFailure, RequestId};
use tendermint_proto::Protobuf;
use tracing::{instrument, trace};

use crate::p2p::P2pError;
use crate::utils::{OneshotResultSender, OneshotResultSenderExt};

pub(crate) struct ExchangeClientHandler {
    reqs: HashMap<RequestId, ReqInfo>,
}

struct ReqInfo {
    amount: u64,
    respond_to: OneshotResultSender<Vec<ExtendedHeader>, P2pError>,
}

impl ExchangeClientHandler {
    pub(crate) fn new() -> Self {
        ExchangeClientHandler {
            reqs: HashMap::new(),
        }
    }

    #[instrument(level = "trace", skip(self, respond_to))]
    pub(crate) fn on_request_initiated(
        &mut self,
        request_id: RequestId,
        amount: u64,
        respond_to: OneshotResultSender<Vec<ExtendedHeader>, P2pError>,
    ) {
        trace!("Request initiated");
        self.reqs.insert(request_id, ReqInfo { amount, respond_to });
    }

    #[instrument(level = "trace", skip(self, responses), fields(responses.len = responses.len()))]
    pub(crate) fn on_response_received(
        &mut self,
        request_id: RequestId,
        responses: Vec<HeaderResponse>,
    ) {
        let Some(ReqInfo { amount, respond_to }) = self.reqs.remove(&request_id) else {
            return;
        };

        trace!("Response received. Expected amount = {amount}");

        // Convert amount to usize
        let Ok(amount) = amount.try_into() else {
            respond_to.maybe_send_err(P2pError::ExchangeHeaderInvalid);
            return;
        };

        if responses.len() != amount {
            // TODO: should we define a separate error for this case?
            respond_to.maybe_send_err(P2pError::ExchangeHeaderInvalid);
            return;
        }

        let mut headers = Vec::with_capacity(amount);

        for response in responses {
            let res = match response.status_code() {
                StatusCode::Invalid => Err(P2pError::ExchangeHeaderInvalid),
                StatusCode::NotFound => Err(P2pError::ExchangeHeaderNotFound),
                StatusCode::Ok => ExtendedHeader::decode(&response.body[..])
                    .map_err(|_| P2pError::ExchangeHeaderInvalid),
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
    pub(crate) fn on_failure(&mut self, request_id: RequestId, error: OutboundFailure) {
        trace!("Outbound failure");

        if let Some(req_info) = self.reqs.remove(&request_id) {
            // TODO: should we actually report a connection error?
            req_info
                .respond_to
                .maybe_send_err(P2pError::ExchangeHeaderInvalid);
        }
    }
}

impl Default for ExchangeClientHandler {
    fn default() -> Self {
        ExchangeClientHandler::new()
    }
}
