//! Clients for the celestia Json-RPC.
//!
//! This module aims to provide a convenient way to create a Json-RPC clients. If
//! you need more configuration options and / or some custom client you can create
//! one using [`jsonrpsee`] crate directly.

#[cfg(not(target_arch = "wasm32"))]
pub use self::native::Client;

#[cfg(target_arch = "wasm32")]
pub use self::wasm::Client;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::fmt;
    use std::result::Result as StdResult;

    use crate::{Error, Result};
    use async_trait::async_trait;
    use http::{header, HeaderValue};
    use jsonrpsee::core::client::{BatchResponse, ClientT, Subscription, SubscriptionClientT};
    use jsonrpsee::core::params::BatchRequestBuilder;
    use jsonrpsee::core::traits::ToRpcParams;
    use jsonrpsee::core::Error as JrpcError;
    use jsonrpsee::http_client::{HeaderMap, HttpClient, HttpClientBuilder};
    use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
    use serde::de::DeserializeOwned;

    /// Json RPC client.
    pub enum Client {
        Http(HttpClient),
        Ws(WsClient),
    }

    impl Client {
        /// Create a new Json RPC client.
        ///
        /// Only 'http[s]' and 'ws[s]' protocols are supported and they should
        /// be specified in the provided `conn_str`. For more flexibility
        /// consider creating the client using [`jsonrpsee`] directly.
        ///
        /// Please note that currently the celestia-node supports only 'http' and 'ws'.
        /// For a secure connection you have to hide it behind a proxy.
        pub async fn new(conn_str: &str, auth_token: Option<&str>) -> Result<Self> {
            let mut headers = HeaderMap::new();

            if let Some(token) = auth_token {
                let val = HeaderValue::from_str(&format!("Bearer {token}"))?;
                headers.insert(header::AUTHORIZATION, val);
            }

            let protocol = conn_str.split_once(':').map(|(proto, _)| proto);
            let client = match protocol {
                Some("http") | Some("https") => Client::Http(
                    HttpClientBuilder::default()
                        .set_headers(headers)
                        .build(conn_str)?,
                ),
                Some("ws") | Some("wss") => Client::Ws(
                    WsClientBuilder::default()
                        .set_headers(headers)
                        .build(conn_str)
                        .await?,
                ),
                _ => return Err(Error::ProtocolNotSupported(conn_str.into())),
            };

            Ok(client)
        }
    }

    #[async_trait]
    impl ClientT for Client {
        async fn notification<Params>(
            &self,
            method: &str,
            params: Params,
        ) -> StdResult<(), JrpcError>
        where
            Params: ToRpcParams + Send,
        {
            match self {
                Client::Http(client) => client.notification(method, params).await,
                Client::Ws(client) => client.notification(method, params).await,
            }
        }

        async fn request<R, Params>(&self, method: &str, params: Params) -> StdResult<R, JrpcError>
        where
            R: DeserializeOwned,
            Params: ToRpcParams + Send,
        {
            match self {
                Client::Http(client) => client.request(method, params).await,
                Client::Ws(client) => client.request(method, params).await,
            }
        }

        async fn batch_request<'a, R>(
            &self,
            batch: BatchRequestBuilder<'a>,
        ) -> StdResult<BatchResponse<'a, R>, JrpcError>
        where
            R: DeserializeOwned + fmt::Debug + 'a,
        {
            match self {
                Client::Http(client) => client.batch_request(batch).await,
                Client::Ws(client) => client.batch_request(batch).await,
            }
        }
    }

    #[async_trait]
    impl SubscriptionClientT for Client {
        async fn subscribe<'a, N, Params>(
            &self,
            subscribe_method: &'a str,
            params: Params,
            unsubscribe_method: &'a str,
        ) -> StdResult<Subscription<N>, JrpcError>
        where
            Params: ToRpcParams + Send,
            N: DeserializeOwned,
        {
            match self {
                Client::Http(client) => {
                    client
                        .subscribe(subscribe_method, params, unsubscribe_method)
                        .await
                }
                Client::Ws(client) => {
                    client
                        .subscribe(subscribe_method, params, unsubscribe_method)
                        .await
                }
            }
        }

        async fn subscribe_to_method<'a, N>(
            &self,
            method: &'a str,
        ) -> StdResult<Subscription<N>, JrpcError>
        where
            N: DeserializeOwned,
        {
            match self {
                Client::Http(client) => client.subscribe_to_method(method).await,
                Client::Ws(client) => client.subscribe_to_method(method).await,
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::fmt;

    use async_trait::async_trait;
    use jsonrpsee::core::client::{BatchResponse, ClientT, Subscription, SubscriptionClientT};
    use jsonrpsee::core::params::BatchRequestBuilder;
    use jsonrpsee::core::traits::ToRpcParams;
    use jsonrpsee::core::Error as JrpcError;
    use serde::de::DeserializeOwned;

    pub struct Client;

    impl Client {
        pub async fn new(_conn_str: &str, _auth_token: Option<&str>) -> crate::Result<Self> {
            // TODO: jsonrpsee_wasm_client doesn't support setting headers
            Ok(Self)
        }
    }

    #[async_trait]
    impl ClientT for Client {
        async fn notification<Params>(
            &self,
            _method: &str,
            _params: Params,
        ) -> Result<(), JrpcError>
        where
            Params: ToRpcParams + Send,
        {
            todo!()
        }

        async fn request<R, Params>(&self, _method: &str, _params: Params) -> Result<R, JrpcError>
        where
            R: DeserializeOwned,
            Params: ToRpcParams + Send,
        {
            todo!()
        }

        async fn batch_request<'a, R>(
            &self,
            _batch: BatchRequestBuilder<'a>,
        ) -> Result<BatchResponse<'a, R>, JrpcError>
        where
            R: DeserializeOwned + fmt::Debug + 'a,
        {
            todo!()
        }
    }

    #[async_trait]
    impl SubscriptionClientT for Client {
        async fn subscribe<'a, N, Params>(
            &self,
            _subscribe_method: &'a str,
            _params: Params,
            _unsubscribe_method: &'a str,
        ) -> Result<Subscription<N>, JrpcError>
        where
            Params: ToRpcParams + Send,
            N: DeserializeOwned,
        {
            todo!()
        }

        async fn subscribe_to_method<'a, N>(
            &self,
            _method: &'a str,
        ) -> Result<Subscription<N>, JrpcError>
        where
            N: DeserializeOwned,
        {
            todo!()
        }
    }

    // TODO: implement HttpClient with `fetch`
}
