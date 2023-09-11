use celestia_proto::p2p::pb::{HeaderRequest, HeaderResponse};
use libp2p::request_response::{InboundFailure, RequestId, ResponseChannel};

pub(crate) struct ExchangeServerHandler {
    // TODO
}

impl ExchangeServerHandler {
    pub(crate) fn new() -> Self {
        ExchangeServerHandler {}
    }

    pub(crate) fn on_request_received(
        &mut self,
        _request_id: RequestId,
        _request: HeaderRequest,
        _respond_to: ResponseChannel<Vec<HeaderResponse>>,
    ) {
        // TODO
    }

    pub(crate) fn on_response_sent(&mut self, _request_id: RequestId) {
        // TODO
    }

    pub(crate) fn on_inbound_failure(&mut self, _request_id: RequestId, _error: InboundFailure) {
        // TODO
    }
}
