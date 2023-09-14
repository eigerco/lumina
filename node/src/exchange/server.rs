use celestia_proto::p2p::pb::{HeaderRequest, HeaderResponse};
use libp2p::request_response::{InboundFailure, RequestId, ResponseChannel};
use tracing::instrument;

pub(super) struct ExchangeServerHandler {
    // TODO
}

impl ExchangeServerHandler {
    pub(super) fn new() -> Self {
        ExchangeServerHandler {}
    }

    #[instrument(level = "trace", skip(self, _respond_to))]
    pub(super) fn on_request_received(
        &mut self,
        request_id: RequestId,
        request: HeaderRequest,
        _respond_to: ResponseChannel<Vec<HeaderResponse>>,
    ) {
        // TODO
    }

    #[instrument(level = "trace", skip(self))]
    pub(super) fn on_response_sent(&mut self, request_id: RequestId) {
        // TODO
    }

    #[instrument(level = "trace", skip(self))]
    pub(super) fn on_failure(&mut self, request_id: RequestId, error: InboundFailure) {
        // TODO
    }
}
