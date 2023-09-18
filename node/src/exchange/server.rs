use celestia_proto::p2p::pb::{HeaderRequest, HeaderResponse};
use libp2p::{
    request_response::{InboundFailure, RequestId, ResponseChannel},
    PeerId,
};
use std::sync::Arc;
use tracing::instrument;

use crate::store::Store;

pub(super) struct ExchangeServerHandler {
    _store: Arc<Store>,
}

impl ExchangeServerHandler {
    pub(super) fn new(store: Arc<Store>) -> Self {
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
