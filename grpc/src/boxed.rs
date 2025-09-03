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

dyn_clone::clone_trait_object!(AbstractTransport);

type BoxedError = Box<dyn StdError + Sync + Send + 'static>;

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
        let ready = ready!(self.poll_frame(cx));

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

trait AbstractTransport: DynClone {
    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), BoxError>>;

    fn call(&mut self, req: http::Request<TonicBody>) -> AbstractFuture;
}

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
    T: Service<http::Request<TonicBody>> + Clone,
    <T as Service<http::Request<TonicBody>>>::Response: AbstractResponse,
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
        Box::pin(Service::call(self, req).map(|response| match response {
            Ok(response) => Ok(response.boxed()),
            Err(e) => Err(box_err(e)),
        }))
    }
}

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
    B: AbstractBody + Send + Unpin + 'static,
    T: Service<http::Request<TonicBody>, Response = http::Response<B>>
        + Send
        + Sync
        + Clone
        + 'static,
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

// conditional send taken from:
// https://github.com/bevyengine/bevy/blob/ad444fa720297080de75b9be1437f3250ac7fc84/crates/bevy_tasks/src/lib.rs
#[cfg(not(target_arch = "wasm32"))]
mod conditional_send {
    /// Use [`ConditionalSend`] to mark an optional Send trait bound. Useful as on certain platforms (eg. WASM),
    /// futures aren't Send.
    pub trait ConditionalSend: Send {}
    impl<T: Send> ConditionalSend for T {}
}

#[cfg(target_arch = "wasm32")]
mod conditional_send {
    /// Use [`ConditionalSend`] to mark an optional Send trait bound. Useful as on certain platforms (eg. WASM),
    /// futures aren't Send.
    pub trait ConditionalSend {}

    impl<T> ConditionalSend for T {}
}

pub use conditional_send::*;

pub trait ConditionalSendFuture: Future + ConditionalSend {}

impl<T: Future + ConditionalSend> ConditionalSendFuture for T {}

/// An owned and dynamically typed Future used when you can't statically type your result or need to add some indirection.
pub type BoxedFuture<'a, T> = Pin<Box<dyn ConditionalSendFuture<Output = T> + 'a>>;
