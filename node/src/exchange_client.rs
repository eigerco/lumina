use std::collections::HashMap;

use celestia_proto::p2p::pb::{HeaderResponse, StatusCode};
use celestia_types::ExtendedHeader;
use libp2p::request_response::{OutboundFailure, RequestId};
use log::trace;
use tendermint_proto::Protobuf;

use crate::exchange::ExchangeError;
use crate::utils::{OneshotResultSender, OneshotSenderExt};

pub(crate) struct ExchangeClientHandler {
    reqs: HashMap<RequestId, OneshotResultSender<ExtendedHeader, ExchangeError>>,
}

impl ExchangeClientHandler {
    pub(crate) fn new() -> Self {
        ExchangeClientHandler {
            reqs: HashMap::new(),
        }
    }

    pub(crate) fn on_request_initiated(
        &mut self,
        request_id: RequestId,
        respond_to: OneshotResultSender<ExtendedHeader, ExchangeError>,
    ) {
        self.reqs.insert(request_id, respond_to);
    }

    pub(crate) fn on_response_received(&mut self, request_id: RequestId, response: HeaderResponse) {
        let res = match response.status_code() {
            StatusCode::Invalid => Err(ExchangeError::UnsupportedHeaderResponse),
            StatusCode::NotFound => Err(ExchangeError::HeaderNotFound),
            StatusCode::Ok => ExtendedHeader::decode(&response.body[..])
                .map_err(|_| ExchangeError::UnsupportedHeaderResponse),
        };

        trace!("Response: {res:?}");

        let respond_to = self.reqs.remove(&request_id).unwrap();
        respond_to.maybe_send(res);
    }

    pub(crate) fn on_failure(&mut self, request_id: RequestId, _error: OutboundFailure) {
        if let Some(respond_to) = self.reqs.remove(&request_id) {
            // TODO: should we actually report a connection error?
            respond_to.maybe_send(Err(ExchangeError::HeaderNotFound));
        }
    }
}

impl Default for ExchangeClientHandler {
    fn default() -> Self {
        ExchangeClientHandler::new()
    }
}
