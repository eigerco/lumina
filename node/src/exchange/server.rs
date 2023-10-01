use std::fmt::{Debug, Display};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use celestia_proto::p2p::pb::{header_request, HeaderRequest, HeaderResponse};
use celestia_types::hash::Hash;
use futures::stream::FuturesUnordered;
use futures::{future::BoxFuture, FutureExt, Stream};
use libp2p::{
    request_response::{InboundFailure, RequestId, ResponseChannel},
    PeerId,
};
use tendermint::hash::Algorithm;
use tracing::{instrument, trace};

use crate::exchange::utils::{ExtendedHeaderExt, HeaderRequestExt, HeaderResponseExt};
use crate::exchange::{ReqRespBehaviour, ResponseType};
use crate::store::Store;

pub(super) struct ExchangeServerHandler<S, R = ReqRespBehaviour>
where
    S: Store,
    R: ResponseSender,
{
    store: Arc<S>,
    futures: FuturesUnordered<BoxFuture<'static, (R::Channel, ResponseType)>>,
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

impl<S, R> ExchangeServerHandler<S, R>
where
    S: Store + 'static,
    R: ResponseSender,
{
    pub(super) fn new(store: Arc<S>) -> Self {
        ExchangeServerHandler {
            store,
            futures: FuturesUnordered::new(),
        }
    }

    #[instrument(level = "trace", skip(self, response_channel))]
    pub(super) fn on_request_received<Id>(
        &mut self,
        peer: PeerId,
        request_id: Id,
        request: HeaderRequest,
        response_channel: R::Channel,
    ) where
        Id: Display + Debug,
    {
        let Some((amount, data)) = parse_request(request) else {
            self.handle_invalid_request(response_channel);
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
                self.handle_request_by_hash(response_channel, hash);
            }
        };
    }

    pub(super) fn on_response_sent(&mut self, peer: PeerId, request_id: RequestId) {
        trace!("response_sent; request_id: {request_id}, peer: {peer}");
    }

    pub(super) fn on_failure(
        &mut self,
        peer: PeerId,
        request_id: RequestId,
        error: InboundFailure,
    ) {
        // TODO: cancel job if libp2p already failed it?
        trace!("on_failure; request_id: {request_id}, peer: {peer}, error: {error:?}");
    }

    pub fn poll(&mut self, cx: &mut Context<'_>, sender: &mut R) -> Poll<()> {
        while let Poll::Ready(Some((channel, response))) = Pin::new(&mut self.futures).poll_next(cx)
        {
            sender.send_response(channel, response);
        }

        Poll::Pending
    }

    fn handle_request_current_head(&mut self, channel: R::Channel) {
        let store = self.store.clone();
        let fut = async move {
            let response = store
                .get_head()
                .await
                .map(|head| head.to_header_response())
                .unwrap_or_else(|_| HeaderResponse::not_found());

            (channel, vec![response])
        };
        self.futures.push(fut.boxed());
    }

    fn handle_request_by_hash(&mut self, channel: R::Channel, hash: Vec<u8>) {
        let store = self.store.clone();
        let fut = async move {
            let Ok(hash) = Hash::from_bytes(Algorithm::Sha256, &hash) else {
                return (channel, vec![HeaderResponse::invalid()]);
            };

            let response = store
                .get_by_hash(&hash)
                .await
                .map(|head| head.to_header_response())
                .unwrap_or_else(|_| HeaderResponse::not_found());

            (channel, vec![response])
        };

        self.futures.push(fut.boxed());
    }

    fn handle_request_by_height(&mut self, channel: R::Channel, origin: u64, amount: u64) {
        let store = self.store.clone();

        let fut = async move {
            let mut responses = vec![];
            for i in origin..origin + amount {
                let response = store
                    .get_by_height(i)
                    .await
                    .map(|head| head.to_header_response())
                    .unwrap_or_else(|_| HeaderResponse::not_found());
                responses.push(response);
            }

            (channel, responses)
        };
        self.futures.push(fut.boxed());
    }

    fn handle_invalid_request(&self, channel: R::Channel) {
        let fut = async { (channel, vec![HeaderResponse::invalid()]) };
        self.futures.push(fut.boxed());
    }
}

fn parse_request(request: HeaderRequest) -> Option<(u64, header_request::Data)> {
    if !request.is_valid() {
        return None;
    }

    let HeaderRequest {
        amount,
        data: Some(data),
    } = request
    else {
        return None;
    };

    Some((amount, data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::utils::HeaderRequestExt;
    use crate::store::tests::gen_filled_store;
    use celestia_proto::p2p::pb::header_request::Data;
    use celestia_proto::p2p::pb::{HeaderRequest, StatusCode};
    use celestia_types::ExtendedHeader;
    use futures::task::noop_waker_ref;
    use libp2p::PeerId;
    use std::sync::Arc;
    use std::task::Context;
    use tendermint_proto::Protobuf;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn request_header_test() {
        let (store, _) = gen_filled_store(3);
        let expected_genesis = store.get_by_height(1).unwrap();
        let mut handler = ExchangeServerHandler::new(Arc::new(store));
        let (mut sender, receiver) = create_test_response_sender();
        let mut cx = create_dummy_async_context();

        handler.on_request_received(
            PeerId::random(),
            "test",
            HeaderRequest::with_origin(1, 1),
            (),
        );

        let _ = handler.poll(&mut cx, &mut sender);

        let received = receiver.await.unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].status_code, i32::from(StatusCode::Ok));
        let decoded_header = ExtendedHeader::decode(&received[0].body[..]).unwrap();
        assert_eq!(decoded_header, expected_genesis);
    }

    #[tokio::test]
    async fn request_head_test() {
        let (store, _) = gen_filled_store(4);
        let expected_head = store.get_head().unwrap();
        let mut handler = ExchangeServerHandler::new(Arc::new(store));
        let (mut sender, receiver) = create_test_response_sender();
        let mut cx = create_dummy_async_context();

        handler.on_request_received(PeerId::random(), "test", HeaderRequest::head_request(), ());

        let _ = handler.poll(&mut cx, &mut sender);

        let received = receiver.await.unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].status_code, i32::from(StatusCode::Ok));
        let decoded_header = ExtendedHeader::decode(&received[0].body[..]).unwrap();
        assert_eq!(decoded_header, expected_head);
    }

    #[tokio::test]
    async fn invalid_amount_request_test() {
        let (store, _) = gen_filled_store(1);
        let mut handler = ExchangeServerHandler::new(Arc::new(store));
        let (mut sender, receiver) = create_test_response_sender();
        let mut cx = create_dummy_async_context();

        handler.on_request_received(
            PeerId::random(),
            "test",
            HeaderRequest::with_origin(0, 0),
            (),
        );

        let _ = handler.poll(&mut cx, &mut sender);

        let received = receiver.await.unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].status_code, i32::from(StatusCode::Invalid));
    }

    #[tokio::test]
    async fn none_data_request_test() {
        let (store, _) = gen_filled_store(1);
        let mut handler = ExchangeServerHandler::new(Arc::new(store));
        let (mut sender, receiver) = create_test_response_sender();
        let mut cx = create_dummy_async_context();

        let request = HeaderRequest {
            data: None,
            amount: 1,
        };
        handler.on_request_received(PeerId::random(), "test", request, ());

        let _ = handler.poll(&mut cx, &mut sender);

        let received = receiver.await.unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].status_code, i32::from(StatusCode::Invalid));
    }

    #[tokio::test]
    async fn request_hash_test() {
        let (store, _) = gen_filled_store(1);
        let stored_header = store.get_head().unwrap();
        let mut handler = ExchangeServerHandler::new(Arc::new(store));
        let (mut sender, receiver) = create_test_response_sender();
        let mut cx = create_dummy_async_context();

        handler.on_request_received(
            PeerId::random(),
            "test",
            HeaderRequest::with_hash(stored_header.hash()),
            (),
        );

        let _ = handler.poll(&mut cx, &mut sender);

        let received = receiver.await.unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].status_code, i32::from(StatusCode::Ok));
        let decoded_header = ExtendedHeader::decode(&received[0].body[..]).unwrap();
        assert_eq!(decoded_header, stored_header);
    }

    #[tokio::test]
    async fn request_range_test() {
        let (store, _) = gen_filled_store(10);
        let expected_headers = [
            store.get_by_height(5).unwrap(),
            store.get_by_height(6).unwrap(),
            store.get_by_height(7).unwrap(),
        ];
        let mut handler = ExchangeServerHandler::new(Arc::new(store));
        let (mut sender, receiver) = create_test_response_sender();
        let mut cx = create_dummy_async_context();

        let request = HeaderRequest {
            data: Some(Data::Origin(5)),
            amount: u64::try_from(expected_headers.len()).unwrap(),
        };
        handler.on_request_received(PeerId::random(), "test", request, ());

        let _ = handler.poll(&mut cx, &mut sender);

        let received = receiver.await.unwrap();

        for (rec, exp) in received.iter().zip(expected_headers.iter()) {
            assert_eq!(rec.status_code, i32::from(StatusCode::Ok));
            let decoded_header = ExtendedHeader::decode(&rec.body[..]).unwrap();
            assert_eq!(&decoded_header, exp);
        }
    }

    #[tokio::test]
    async fn request_range_behond_head_test() {
        let (store, _) = gen_filled_store(5);
        let expected_hashes = [store.get_by_height(5).ok(), None, None];
        let expected_status_codes = [StatusCode::Ok, StatusCode::NotFound, StatusCode::NotFound];
        assert_eq!(expected_hashes.len(), expected_status_codes.len());

        let mut handler = ExchangeServerHandler::new(Arc::new(store));

        let (mut sender, receiver) = create_test_response_sender();
        let mut cx = create_dummy_async_context();

        let request = HeaderRequest::with_origin(5, u64::try_from(expected_hashes.len()).unwrap());
        handler.on_request_received(PeerId::random(), "test", request, ());

        let _ = handler.poll(&mut cx, &mut sender);

        let received = receiver.await.unwrap();
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
    struct TestResponseSender(pub Option<oneshot::Sender<ResponseType>>);

    impl ResponseSender for TestResponseSender {
        type Channel = ();

        fn send_response(&mut self, _channel: Self::Channel, response: ResponseType) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(response);
            }
        }
    }

    fn create_test_response_sender() -> (TestResponseSender, oneshot::Receiver<ResponseType>) {
        let (tx, rx) = oneshot::channel();
        (TestResponseSender(Some(tx)), rx)
    }

    fn create_dummy_async_context() -> Context<'static> {
        let waker = noop_waker_ref();
        Context::from_waker(waker)
    }
}
