use std::fmt::{Debug, Display};
use std::sync::Arc;
use std::task::{Context, Poll};

use celestia_proto::p2p::pb::{header_request, HeaderRequest, HeaderResponse};
use celestia_types::hash::Hash;
use futures::future::BoxFuture;
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt};
use libp2p::{
    request_response::{InboundFailure, InboundRequestId, ResponseChannel},
    PeerId,
};
use tracing::{instrument, trace};

use crate::p2p::header_ex::utils::{ExtendedHeaderExt, HeaderRequestExt, HeaderResponseExt};
use crate::p2p::header_ex::{ReqRespBehaviour, ResponseType};
use crate::store::Store;

const MAX_HEADERS_AMOUNT_RESPONSE: u64 = 512;

pub(super) struct HeaderExServerHandler<S, R = ReqRespBehaviour>
where
    S: Store,
    R: ResponseSender,
{
    store: Arc<S>,
    stopping: bool,
    tasks: FuturesUnordered<BoxFuture<'static, (R::Channel, ResponseType)>>,
}

pub(super) trait ResponseSender {
    type Channel: Send + 'static;

    fn send_response(&mut self, channel: Self::Channel, response: ResponseType);
}

impl ResponseSender for ReqRespBehaviour {
    type Channel = ResponseChannel<ResponseType>;

    fn send_response(&mut self, channel: Self::Channel, response: ResponseType) {
        // response was prepared specifically for the request, we can drop it
        // in case of error we'll get Event::InboundFailure
        let _ = self.send_response(channel, response);
    }
}

impl<S, R> HeaderExServerHandler<S, R>
where
    S: Store + 'static,
    R: ResponseSender,
{
    pub(super) fn new(store: Arc<S>) -> Self {
        HeaderExServerHandler {
            store,
            stopping: false,
            tasks: FuturesUnordered::new(),
        }
    }

    #[instrument(level = "trace", skip(self, response_sender, response_channel))]
    pub(super) fn on_request_received<Id>(
        &mut self,
        peer: PeerId,
        request_id: Id,
        request: HeaderRequest,
        response_sender: &mut R,
        response_channel: R::Channel,
    ) where
        Id: Display + Debug,
    {
        if self.stopping {
            return;
        }

        let Some((amount, data)) = parse_request(request) else {
            self.handle_invalid_request(response_sender, response_channel);
            return;
        };

        match data {
            header_request::Data::Origin(0) => {
                self.handle_request_current_head(response_channel);
            }
            header_request::Data::Origin(height) => {
                self.handle_request_by_height(response_channel, height, amount);
            }
            header_request::Data::Hash(hash) => {
                self.handle_request_by_hash(response_sender, response_channel, hash);
            }
        };
    }

    pub(super) fn on_response_sent(&mut self, peer: PeerId, request_id: InboundRequestId) {
        trace!("response_sent; request_id: {request_id}, peer: {peer}");
    }

    pub(super) fn on_failure(
        &mut self,
        peer: PeerId,
        request_id: InboundRequestId,
        error: InboundFailure,
    ) {
        // TODO: cancel job if libp2p already failed it?
        trace!("on_failure; request_id: {request_id}, peer: {peer}, error: {error:?}");
    }

    pub(super) fn on_stop(&mut self) {
        self.stopping = true;
        self.tasks.clear();
    }

    fn handle_request_current_head(&mut self, channel: R::Channel) {
        let store = self.store.clone();

        self.tasks.push(
            async move {
                let response = store
                    .get_head()
                    .await
                    .map(|head| head.to_header_response())
                    .unwrap_or_else(|_| HeaderResponse::not_found());

                (channel, vec![response])
            }
            .boxed(),
        );
    }

    fn handle_request_by_hash(&mut self, sender: &mut R, channel: R::Channel, hash: Vec<u8>) {
        let Ok(hash) = hash.try_into().map(Hash::Sha256) else {
            self.handle_invalid_request(sender, channel);
            return;
        };

        let store = self.store.clone();

        self.tasks.push(
            async move {
                let response = store
                    .get_by_hash(&hash)
                    .await
                    .map(|head| head.to_header_response())
                    .unwrap_or_else(|_| HeaderResponse::not_found());

                (channel, vec![response])
            }
            .boxed(),
        );
    }

    fn handle_request_by_height(&mut self, channel: R::Channel, origin: u64, amount: u64) {
        let store = self.store.clone();

        self.tasks.push(
            async move {
                let amount = amount.min(MAX_HEADERS_AMOUNT_RESPONSE);
                let mut responses = vec![];

                for i in origin..origin + amount {
                    match store.get_by_height(i).await {
                        Ok(h) => {
                            if responses.is_empty() {
                                responses.reserve_exact(amount as usize);
                            }

                            responses.push(h.to_header_response());
                        }
                        Err(_) => break,
                    }
                }

                if responses.is_empty() {
                    responses.reserve_exact(1);
                    responses.push(HeaderResponse::not_found());
                }

                (channel, responses)
            }
            .boxed(),
        );
    }

    fn handle_invalid_request(&self, sender: &mut R, channel: R::Channel) {
        sender.send_response(channel, vec![HeaderResponse::invalid()]);
    }

    pub fn poll(&mut self, cx: &mut Context<'_>, sender: &mut R) -> Poll<()> {
        if let Poll::Ready(Some((channel, response))) = self.tasks.poll_next_unpin(cx) {
            sender.send_response(channel, response);
            return Poll::Ready(());
        }

        Poll::Pending
    }
}

fn parse_request(request: HeaderRequest) -> Option<(u64, header_request::Data)> {
    if !request.is_valid() {
        return None;
    }

    request.data.map(|data| (request.amount, data))
}

#[cfg(test)]
mod tests {
    use super::{ResponseSender, *};
    use crate::store::InMemoryStore;
    use crate::test_utils::{async_test, gen_filled_store};
    use celestia_proto::p2p::pb::header_request::Data;
    use celestia_proto::p2p::pb::StatusCode;
    use celestia_tendermint_proto::Protobuf;
    use celestia_types::ExtendedHeader;
    use std::future::poll_fn;
    use tokio::select;
    use tokio::sync::oneshot;

    #[async_test]
    async fn request_head_test() {
        let (store, _) = gen_filled_store(4).await;
        let expected_head = store.get_head().await.unwrap();
        let (mut handler, mut sender) = mocked_server_handler(store);

        let (tx, rx) = oneshot::channel();
        handler.on_request_received(
            PeerId::random(),
            "test",
            HeaderRequest::head_request(),
            &mut sender,
            tx,
        );

        let received = poll_handler_for_result(&mut handler, &mut sender, rx).await;

        assert_eq!(received.len(), 1);
        assert_eq!(received[0].status_code, i32::from(StatusCode::Ok));
        let decoded_header = ExtendedHeader::decode(&received[0].body[..]).unwrap();
        assert_eq!(decoded_header, expected_head);
    }

    #[async_test]
    async fn request_header_test() {
        let (store, _) = gen_filled_store(3).await;
        let expected_genesis = store.get_by_height(1).await.unwrap();
        let (mut handler, mut sender) = mocked_server_handler(store);

        let (tx, rx) = oneshot::channel();
        handler.on_request_received(
            PeerId::random(),
            "test",
            HeaderRequest::with_origin(1, 1),
            &mut sender,
            tx,
        );

        let received = poll_handler_for_result(&mut handler, &mut sender, rx).await;

        assert_eq!(received.len(), 1);
        assert_eq!(received[0].status_code, i32::from(StatusCode::Ok));
        let decoded_header = ExtendedHeader::decode(&received[0].body[..]).unwrap();
        assert_eq!(decoded_header, expected_genesis);
    }

    #[async_test]
    async fn invalid_amount_request_test() {
        let (store, _) = gen_filled_store(1).await;
        let (mut handler, mut sender) = mocked_server_handler(store);

        let (tx, rx) = oneshot::channel();
        handler.on_request_received(
            PeerId::random(),
            "test",
            HeaderRequest::with_origin(0, 0),
            &mut sender,
            tx,
        );

        let received = poll_handler_for_result(&mut handler, &mut sender, rx).await;

        assert_eq!(received.len(), 1);
        assert_eq!(received[0].status_code, i32::from(StatusCode::Invalid));
    }

    #[async_test]
    async fn none_data_request_test() {
        let (store, _) = gen_filled_store(1).await;
        let (mut handler, mut sender) = mocked_server_handler(store);

        let request = HeaderRequest {
            data: None,
            amount: 1,
        };
        let (tx, rx) = oneshot::channel();
        handler.on_request_received(PeerId::random(), "test", request, &mut sender, tx);

        let received = poll_handler_for_result(&mut handler, &mut sender, rx).await;

        assert_eq!(received.len(), 1);
        assert_eq!(received[0].status_code, i32::from(StatusCode::Invalid));
    }

    #[async_test]
    async fn request_hash_test() {
        let (store, _) = gen_filled_store(1).await;
        let stored_header = store.get_head().await.unwrap();
        let (mut handler, mut sender) = mocked_server_handler(store);

        let (tx, rx) = oneshot::channel();
        handler.on_request_received(
            PeerId::random(),
            "test",
            HeaderRequest::with_hash(stored_header.hash()),
            &mut sender,
            tx,
        );

        let received = poll_handler_for_result(&mut handler, &mut sender, rx).await;

        assert_eq!(received.len(), 1);
        assert_eq!(received[0].status_code, i32::from(StatusCode::Ok));
        let decoded_header = ExtendedHeader::decode(&received[0].body[..]).unwrap();
        assert_eq!(decoded_header, stored_header);
    }

    #[async_test]
    async fn request_malformed_hash_test() {
        let (store, _) = gen_filled_store(1).await;
        let (mut handler, mut sender) = mocked_server_handler(store);

        let request = HeaderRequest {
            data: Some(header_request::Data::Hash(vec![0; 31])),
            amount: 1,
        };

        let (tx, rx) = oneshot::channel();
        handler.on_request_received(PeerId::random(), "test", request, &mut sender, tx);

        let received = poll_handler_for_result(&mut handler, &mut sender, rx).await;

        assert_eq!(received.len(), 1);
        assert_eq!(received[0].status_code, i32::from(StatusCode::Invalid));
    }

    #[async_test]
    async fn request_range_test() {
        let (store, _) = gen_filled_store(10).await;
        let expected_headers = [
            store.get_by_height(5).await.unwrap(),
            store.get_by_height(6).await.unwrap(),
            store.get_by_height(7).await.unwrap(),
        ];
        let (mut handler, mut sender) = mocked_server_handler(store);

        let request = HeaderRequest {
            data: Some(Data::Origin(5)),
            amount: u64::try_from(expected_headers.len()).unwrap(),
        };
        let (tx, rx) = oneshot::channel();
        handler.on_request_received(PeerId::random(), "test", request, &mut sender, tx);

        let received = poll_handler_for_result(&mut handler, &mut sender, rx).await;

        for (rec, exp) in received.iter().zip(expected_headers.iter()) {
            assert_eq!(rec.status_code, i32::from(StatusCode::Ok));
            let decoded_header = ExtendedHeader::decode(&rec.body[..]).unwrap();
            assert_eq!(&decoded_header, exp);
        }
    }

    #[async_test]
    async fn request_range_beyond_head_test() {
        let (store, _) = gen_filled_store(5).await;
        let expected_hashes = [store.get_by_height(5).await.ok()];
        let expected_status_codes = [StatusCode::Ok];
        assert_eq!(expected_hashes.len(), expected_status_codes.len());

        let (mut handler, mut sender) = mocked_server_handler(store);

        let request = HeaderRequest::with_origin(5, 10);
        let (tx, rx) = oneshot::channel();
        handler.on_request_received(PeerId::random(), "test", request, &mut sender, tx);

        let received = poll_handler_for_result(&mut handler, &mut sender, rx).await;

        assert_eq!(received.len(), expected_hashes.len());
        for (rec, (exp_status, exp_header)) in received
            .iter()
            .zip(expected_status_codes.iter().zip(expected_hashes.iter()))
        {
            assert_eq!(rec.status_code, i32::from(*exp_status));
            if let Some(exp_header) = exp_header {
                let decoded_header = ExtendedHeader::decode(&rec.body[..]).unwrap();
                assert_eq!(&decoded_header, exp_header);
            }
        }
    }

    #[derive(Debug)]
    struct TestResponseSender;

    impl ResponseSender for TestResponseSender {
        type Channel = oneshot::Sender<ResponseType>;

        fn send_response(&mut self, channel: Self::Channel, response: ResponseType) {
            let _ = channel.send(response);
        }
    }

    fn mocked_server_handler(
        store: InMemoryStore,
    ) -> (
        HeaderExServerHandler<InMemoryStore, TestResponseSender>,
        TestResponseSender,
    ) {
        (
            HeaderExServerHandler::new(Arc::new(store)),
            TestResponseSender,
        )
    }

    // helper which waits for result over the test channel, while continously polling the handler.
    async fn poll_handler_for_result<S>(
        handler: &mut HeaderExServerHandler<S, TestResponseSender>,
        response_sender: &mut TestResponseSender,
        mut response_channel: oneshot::Receiver<ResponseType>,
    ) -> Vec<HeaderResponse>
    where
        S: Store + 'static,
    {
        loop {
            select! {
                _ = poll_fn(|cx| handler.poll(cx, response_sender)) => continue,
                r = &mut response_channel => return r.unwrap(),
            }
        }
    }
}
