use std::ops::DerefMut;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{error::Error as StdError, future::Future};

use bytes::Bytes;
use dyn_clone::{clone_box, DynClone};
use futures::FutureExt;
use http_body::Frame;
use tonic::body::Body as TonicBody;
use tonic::codegen::Service;

type BoxError = Box<dyn StdError + Sync + Send + 'static>;

pub(crate) struct BoxedBody {
    inner: Box<dyn AbstractBody + Unpin + Send + 'static>,
}

impl http_body::Body for BoxedBody {
    type Data = Bytes;

    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut BoxedBody>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        AbstractBody::poll_frame_inner(Pin::new(self.inner.deref_mut()), cx)
    }
}

pub trait AbstractBody {
    fn poll_frame_inner(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>>;
}

/*
impl<T> AbstractBody for Box<T> where T: AbstractBody {
    fn poll_frame_inner(self: Pin<&mut Self>, cx: &mut Context<'_>)
        -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
            self.poll_frame_inner(cx)
    }
}
*/

#[cfg(not(target_arch = "wasm32"))]
impl<T> AbstractBody for T
where
    T: tonic::transport::Body<Data = Bytes> + Send + 'static,
    <T as tonic::transport::Body>::Error: StdError + Send + Sync + 'static,
{
    fn poll_frame_inner(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        let Poll::Ready(ready) = tonic::transport::Body::poll_frame(self, cx) else {
            return Poll::Pending;
        };
        let Some(result) = ready else {
            return Poll::Ready(None);
        };

        Poll::Ready(Some(result.map_err(box_err)))
    }
}

#[cfg(target_arch = "wasm32")]
impl<T> AbstractBody for T
where
    T: http_body::Body<Data = Bytes>,
    <T as http_body::Body>::Error: StdError + Send + Sync + 'static,
{
    fn poll_frame_inner(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        let Poll::Ready(ready) = self.poll_frame(cx) else {
            return Poll::Pending;
        };

        let Some(result) = ready else {
            return Poll::Ready(None);
        };

        Poll::Ready(Some(result.map_err(box_err)))
    }
}

pub(crate) struct BoxedTransport {
    inner: Box<dyn AbstractTransport + Send + Sync>,
}

impl Clone for BoxedTransport {
    fn clone(&self) -> Self {
        Self {
            inner: clone_box(&*self.inner),
        }
    }
}

dyn_clone::clone_trait_object!(AbstractTransport);

trait AbstractTransport: DynClone {
    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), BoxError>>;

    fn call(&mut self, req: http::Request<TonicBody>) -> AbstractFuture;
}

// https://github.com/bevyengine/bevy/blob/ad444fa720297080de75b9be1437f3250ac7fc84/crates/bevy_tasks/src/lib.rs
#[cfg(not(target_arch = "wasm32"))]
mod conditional_send {
    /// Use [`ConditionalSend`] to mark an optional Send trait bound. Useful as on certain platforms (eg. WASM),
    /// futures aren't Send.
    pub trait ConditionalSend: Send {}
    impl<T: Send> ConditionalSend for T {}
}

/*
pub trait AbstractResponseBound {
    fn boxed(self) -> http::Response<BoxBody>;
}
#[cfg(not(target_arch = "wasm32"))]
impl<T> AbstractResponseBound for http::Response<T> where T: tonic::transport::Body {
    fn boxed(self) -> http::Response<BoxBody> {

    }
}

#[cfg(target_arch = "wasm32")]
impl<T> AbstractResponseBound for http::Response<T>
where
    T: http_body::Body<Data = Bytes> + Send + Sync + Unpin ,
    //<T as http_body::Body>::Data : Bytes,
{
    fn boxed(self) -> http::Response<BoxBody> {
        self.map(|body| Box::new(body) )
    }
}
*/

/*
pub trait AbstractBody {}
//#[cfg(not(target_arch = "wasm32"))]
impl<T> AbstractBody for T where T: tonic::transport::Body {}
#[cfg(target_arch = "wasm32")]
impl<T> AbstractBody for T where T: http_body::Body {}
*/

trait AbstractResponse {
    fn boxed(self) -> http::Response<BoxedBody>;
}

impl<B> AbstractResponse for http::Response<B>
where
    B: AbstractBody + Unpin + Send + 'static,
{
    fn boxed(self) -> http::Response<BoxedBody> {
        self.map(boxed_body)
    }
}

impl<T> AbstractTransport for T
where
    //T: Service<http::Request<TonicBody>, Response = http::Response<B>> + Clone,
    T: Service<http::Request<TonicBody>> + Clone,
    <T as Service<http::Request<TonicBody>>>::Response: AbstractResponse, // + Clone,
    <T as Service<http::Request<TonicBody>>>::Error: StdError + Send + Sync + 'static,
    <T as Service<http::Request<TonicBody>>>::Future: ConditionalSend + 'static,
{
    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), BoxError>> {
        Service::poll_ready(self, cx).map_err(box_err)
    }

    fn call(&mut self, req: http::Request<TonicBody>) -> AbstractFuture {
        //Box::pin(Service::call(self, req).map_err(box_err))
        Box::pin(Service::call(self, req).map(|response| match response {
            Ok(response) => Ok(response.boxed()),
            Err(e) => Err(box_err(e)),
        }))
    }
}

//type AbstractResponse<B> = http::Response<B>;
//type AbstractError = BoxError;
type BoxedResponse = http::Response<BoxedBody>;
type AbstractFuture = BoxedFuture<'static, Result<BoxedResponse, BoxError>>;

impl Service<http::Request<TonicBody>> for BoxedTransport {
    type Response = http::Response<BoxedBody>;
    type Error = BoxError;
    type Future = AbstractFuture;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<TonicBody>) -> Self::Future {
        self.inner.call(req)
    }
}

pub(crate) fn boxed<T, B>(transport: T) -> BoxedTransport
where
    B: AbstractBody + Send + Unpin + 'static, // TODO :rmeove?
    T: Service<http::Request<TonicBody>, Response = http::Response<B>>
        + Send
        + Sync
        //+ Unpin
        + Clone
        + 'static,
    //<T as Service<http::Request<TonicBody>>>::Response: Body + Send,
    <T as Service<http::Request<TonicBody>>>::Error: StdError + Send + Sync + 'static,
    <T as Service<http::Request<TonicBody>>>::Future: ConditionalSend + 'static,
{
    BoxedTransport {
        inner: Box::new(transport),
    }
}

fn boxed_body<B>(body: B) -> BoxedBody
where
    B: AbstractBody + Send + Unpin + 'static,
{
    BoxedBody {
        inner: Box::new(body),
    }
}

fn box_err<E: StdError + Send + Sync + 'static>(e: E) -> BoxError {
    Box::new(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use celestia_proto::cosmos::base::tendermint::v1beta1::service_client::ServiceClient;

    #[cfg(not(target_arch = "wasm32"))]
    fn can_box_endpoint() {
        let channel = tonic::transport::Endpoint::from_shared("foo")
            .unwrap()
            .connect_lazy();

        let mut client = ServiceClient::new(boxed(channel));
        let _ = client.get_latest_block(GetLatestBlockRequest {});
    }

    #[cfg(target_arch = "wasm32")]
    fn can_box_wasm_client() {
        let channel = tonic_web_wasm_client::Client::new("foo".to_string());

        let mut client = ServiceClient::new(boxed(channel));
        let _ = client.get_latest_block(GetLatestBlockRequest {});
    }
}

#[cfg(target_arch = "wasm32")]
#[allow(missing_docs)]
mod conditional_send {
    pub trait ConditionalSend {}
    impl<T> ConditionalSend for T {}
}

pub use conditional_send::*;

pub trait ConditionalSendFuture: Future + ConditionalSend {}

impl<T: Future + ConditionalSend> ConditionalSendFuture for T {}

/// An owned and dynamically typed Future used when you can't statically type your result or need to add some indirection.
pub type BoxedFuture<'a, T> = core::pin::Pin<Box<dyn ConditionalSendFuture<Output = T> + 'a>>;
