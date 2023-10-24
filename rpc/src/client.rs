//! Clients for the celestia Json-RPC.
//!
//! This module aims to provide a convenient way to create a Json-RPC clients. If
//! you need more configuration options and / or some custom client you can create
//! one using [`jsonrpsee`] crate directly.

#[cfg(not(target_arch = "wasm32"))]
pub use self::imp::{HttpClient, WsClient};

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use crate::Result;
    use http::{header, HeaderValue};
    use jsonrpsee::http_client::{HeaderMap, HttpClient as JrpcHttpClient, HttpClientBuilder};
    use jsonrpsee::ws_client::{WsClient as JrpcWsClient, WsClientBuilder};

    /// Json RPC client using http protocol.
    pub struct HttpClient {
        inner: JrpcHttpClient,
    }

    impl HttpClient {
        /// Create a new http client.
        pub fn new(conn_str: &str, auth_token: Option<&str>) -> Result<Self> {
            let mut headers = HeaderMap::new();

            if let Some(token) = auth_token {
                let val = HeaderValue::from_str(&format!("Bearer {token}"))?;
                headers.insert(header::AUTHORIZATION, val);
            }

            let client = HttpClientBuilder::default()
                .set_headers(headers)
                .build(conn_str)?;

            Ok(Self { inner: client })
        }
    }

    /// Json RPC client using websockets protocol.
    pub struct WsClient {
        inner: JrpcWsClient,
    }

    impl WsClient {
        /// Create a new websocket client.
        pub async fn new(conn_str: &str, auth_token: Option<&str>) -> Result<Self> {
            let mut headers = HeaderMap::new();

            if let Some(token) = auth_token {
                let val = HeaderValue::from_str(&format!("Bearer {token}"))?;
                headers.insert(header::AUTHORIZATION, val);
            }

            let client = WsClientBuilder::default()
                .set_headers(headers)
                .build(conn_str)
                .await?;

            Ok(Self { inner: client })
        }
    }

    /// Implements `ClientT` and `SubscriptionClientT` by delegating calls to $typ::$target.
    macro_rules! impl_jsonrpc_client_delegate {
        ($typ:ty, $target:ident) => {
            #[::async_trait::async_trait]
            impl ::jsonrpsee::core::client::ClientT for $typ {
                async fn notification<Params>(
                    &self,
                    method: &str,
                    params: Params,
                ) -> ::std::result::Result<(), ::jsonrpsee::core::Error>
                where
                    Params: ::jsonrpsee::core::traits::ToRpcParams + ::std::marker::Send,
                {
                    self.$target.notification(method, params).await
                }

                async fn request<R, Params>(
                    &self,
                    method: &str,
                    params: Params,
                ) -> ::std::result::Result<R, ::jsonrpsee::core::Error>
                where
                    R: ::serde::de::DeserializeOwned,
                    Params: ::jsonrpsee::core::traits::ToRpcParams + ::std::marker::Send,
                {
                    self.$target.request(method, params).await
                }

                async fn batch_request<'a, R>(
                    &self,
                    batch: ::jsonrpsee::core::params::BatchRequestBuilder<'a>,
                ) -> ::std::result::Result<
                    ::jsonrpsee::core::client::BatchResponse<'a, R>,
                    ::jsonrpsee::core::Error,
                >
                where
                    R: ::serde::de::DeserializeOwned + ::std::fmt::Debug + 'a,
                {
                    self.$target.batch_request(batch).await
                }
            }

            #[::async_trait::async_trait]
            impl ::jsonrpsee::core::client::SubscriptionClientT for $typ {
                async fn subscribe<'a, N, Params>(
                    &self,
                    subscribe_method: &'a str,
                    params: Params,
                    unsubscribe_method: &'a str,
                ) -> ::std::result::Result<
                    ::jsonrpsee::core::client::Subscription<N>,
                    ::jsonrpsee::core::Error,
                >
                where
                    Params: ::jsonrpsee::core::traits::ToRpcParams + ::std::marker::Send,
                    N: ::serde::de::DeserializeOwned,
                {
                    self.$target
                        .subscribe(subscribe_method, params, unsubscribe_method)
                        .await
                }

                async fn subscribe_to_method<'a, N>(
                    &self,
                    method: &'a str,
                ) -> ::std::result::Result<
                    ::jsonrpsee::core::client::Subscription<N>,
                    ::jsonrpsee::core::Error,
                >
                where
                    N: ::serde::de::DeserializeOwned,
                {
                    self.$target.subscribe_to_method(method).await
                }
            }
        };
    }

    impl_jsonrpc_client_delegate!(HttpClient, inner);
    impl_jsonrpc_client_delegate!(WsClient, inner);
}

#[cfg(target_arch = "wasm32")]
mod imp {
    // TODO: implement HttpClient with `fetch`
}
