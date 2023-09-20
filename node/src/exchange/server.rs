use std::sync::Arc;

use celestia_proto::p2p::pb::{HeaderRequest, HeaderResponse};
use libp2p::{
    request_response::{InboundFailure, RequestId, ResponseChannel},
    PeerId,
};
use tracing::instrument;

use crate::store::BoxedStore;

pub(super) struct ExchangeServerHandler {
    _store: Arc<BoxedStore>,
}

impl ExchangeServerHandler {
    pub(super) fn new(store: Arc<BoxedStore>) -> Self {
        ExchangeServerHandler { _store: store }
    }

    #[instrument(level = "trace", skip(self, _respond_to))]
    pub(super) fn on_request_received(
        &mut self,
        peer: PeerId,
        request_id: RequestId,
        request: HeaderRequest,
        _respond_to: ResponseChannel<Vec<HeaderResponse>>,
    ) {
        // TODO
    }

    #[instrument(level = "trace", skip(self))]
    pub(super) fn on_response_sent(&mut self, peer: PeerId, request_id: RequestId) {
        // TODO
    }

    #[instrument(level = "trace", skip(self))]
    pub(super) fn on_failure(
        &mut self,
        peer: PeerId,
        request_id: RequestId,
        error: InboundFailure,
    ) {
        // TODO
    }
}
