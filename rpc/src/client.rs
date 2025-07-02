//! Clients for the celestia Json-RPC.
//!
//! This module aims to provide a convenient way to create a Json-RPC clients. If
//! you need more configuration options and / or some custom client you can create
//! one using [`jsonrpsee`] crate directly.

#[cfg(not(target_arch = "wasm32"))]
pub use self::native::Client;

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub use self::wasm::Client;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::fmt;
    use std::result::Result;

    use celestia_types::consts::appconsts::{self, SHARE_SIZE};
    use http::{header, HeaderValue};
    use jsonrpsee::core::client::{BatchResponse, ClientT, Subscription, SubscriptionClientT};
    use jsonrpsee::core::params::BatchRequestBuilder;
    use jsonrpsee::core::traits::ToRpcParams;
    use jsonrpsee::core::ClientError;
    use jsonrpsee::http_client::{HeaderMap, HttpClient, HttpClientBuilder};
    use jsonrpsee::ws_client::{WsClient, WsClientBuilder};
    use serde::de::DeserializeOwned;

    use crate::Error;

    // NOTE: Always the largest `appconsts::*::SQUARE_SIZE_UPPER_BOUND` needs to be used.
    const MAX_EDS_SIZE_BYTES: usize = appconsts::v3::SQUARE_SIZE_UPPER_BOUND
        * appconsts::v3::SQUARE_SIZE_UPPER_BOUND
        * 4
        * SHARE_SIZE;

    // The biggest response we might get is for requesting an EDS.
    // Also, we allow 1 MB extra for any metadata they come with it.
    const MAX_RESPONSE_SIZE: usize = MAX_EDS_SIZE_BYTES + 1024 * 1024;

    /// Json RPC client.
    pub enum Client {
        /// A client using 'http\[s\]' protocol.
        Http(HttpClient),
        /// A client using 'ws\[s\]' protocol.
        Ws(WsClient),
    }

    impl Client {
        /// Create a new Json RPC client.
        ///
        /// Only 'http\[s\]' and 'ws\[s\]' protocols are supported and they should
        /// be specified in the provided `conn_str`. For more flexibility
        /// consider creating the client using [`jsonrpsee`] directly.
        ///
        /// Please note that currently the celestia-node supports only 'http' and 'ws'.
        /// For a secure connection you have to hide it behind a proxy.
        pub async fn new(conn_str: &str, auth_token: Option<&str>) -> Result<Self, Error> {
            let mut headers = HeaderMap::new();

            if let Some(token) = auth_token {
                let val = HeaderValue::from_str(&format!("Bearer {token}"))?;
                headers.insert(header::AUTHORIZATION, val);
            }

            let protocol = conn_str.split_once(':').map(|(proto, _)| proto);
            let client = match protocol {
                Some("http") | Some("https") => Client::Http(
                    HttpClientBuilder::default()
                        .max_response_size(MAX_RESPONSE_SIZE as u32)
                        .set_headers(headers)
                        .build(conn_str)?,
                ),
                Some("ws") | Some("wss") => Client::Ws(
                    WsClientBuilder::default()
                        .max_response_size(MAX_RESPONSE_SIZE as u32)
                        .set_headers(headers)
                        .build(conn_str)
                        .await?,
                ),
                _ => return Err(Error::ProtocolNotSupported(conn_str.into())),
            };

            Ok(client)
        }
    }

    impl ClientT for Client {
        async fn notification<Params>(
            &self,
            method: &str,
            params: Params,
        ) -> Result<(), ClientError>
        where
            Params: ToRpcParams + Send,
        {
            match self {
                Client::Http(client) => client.notification(method, params).await,
                Client::Ws(client) => client.notification(method, params).await,
            }
        }

        async fn request<R, Params>(&self, method: &str, params: Params) -> Result<R, ClientError>
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
        ) -> Result<BatchResponse<'a, R>, ClientError>
        where
            R: DeserializeOwned + fmt::Debug + 'a,
        {
            match self {
                Client::Http(client) => client.batch_request(batch).await,
                Client::Ws(client) => client.batch_request(batch).await,
            }
        }
    }

    impl SubscriptionClientT for Client {
        async fn subscribe<'a, N, Params>(
            &self,
            subscribe_method: &'a str,
            params: Params,
            unsubscribe_method: &'a str,
        ) -> Result<Subscription<N>, ClientError>
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

        async fn subscribe_to_method<N>(&self, method: &str) -> Result<Subscription<N>, ClientError>
        where
            N: DeserializeOwned,
        {
            match self {
                Client::Http(client) => client.subscribe_to_method(method).await,
                Client::Ws(client) => client.subscribe_to_method(method).await,
            }
        }
    }

    impl fmt::Debug for Client {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("Client { .. }")
        }
    }
}

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
mod wasm {
    use std::fmt;
    use std::result::Result;

    use jsonrpsee::core::client::{BatchResponse, ClientT, Subscription, SubscriptionClientT};
    use jsonrpsee::core::params::BatchRequestBuilder;
    use jsonrpsee::core::traits::ToRpcParams;
    use jsonrpsee::core::ClientError;
    use jsonrpsee::wasm_client::{Client as WasmClient, WasmClientBuilder};
    use serde::de::DeserializeOwned;

    use crate::Error;

    /// Json RPC client.
    pub struct Client {
        client: WasmClient,
    }

    impl Client {
        /// Create a new Json RPC client.
        ///
        /// Only the 'ws\[s\]' protocols are supported and they should
        /// be specified in the provided `conn_str`. For more flexibility
        /// consider creating the client using [`jsonrpsee`] directly.
        ///
        /// Since headers are not supported in the current version of
        /// `jsonrpsee-wasm-client`, celestia-node requires disabling
        /// authentication (--rpc.skip-auth) to use wasm.
        ///
        /// For a secure connection you have to hide it behind a proxy.
        pub async fn new(conn_str: &str) -> Result<Self, Error> {
            let protocol = conn_str.split_once(':').map(|(proto, _)| proto);
            let client = match protocol {
                Some("ws") | Some("wss") => WasmClientBuilder::default().build(conn_str).await?,
                _ => return Err(Error::ProtocolNotSupported(conn_str.into())),
            };

            Ok(Client { client })
        }
    }

    impl ClientT for Client {
        async fn notification<Params>(
            &self,
            method: &str,
            params: Params,
        ) -> Result<(), ClientError>
        where
            Params: ToRpcParams + Send,
        {
            self.client.notification(method, params).await
        }

        async fn request<R, Params>(&self, method: &str, params: Params) -> Result<R, ClientError>
        where
            R: DeserializeOwned,
            Params: ToRpcParams + Send,
        {
            self.client.request(method, params).await
        }

        async fn batch_request<'a, R>(
            &self,
            batch: BatchRequestBuilder<'a>,
        ) -> Result<BatchResponse<'a, R>, ClientError>
        where
            R: DeserializeOwned + fmt::Debug + 'a,
        {
            self.client.batch_request(batch).await
        }
    }

    impl SubscriptionClientT for Client {
        async fn subscribe<'a, N, Params>(
            &self,
            subscribe_method: &'a str,
            params: Params,
            unsubscribe_method: &'a str,
        ) -> Result<Subscription<N>, ClientError>
        where
            Params: ToRpcParams + Send,
            N: DeserializeOwned,
        {
            self.client
                .subscribe(subscribe_method, params, unsubscribe_method)
                .await
        }

        async fn subscribe_to_method<N>(&self, method: &str) -> Result<Subscription<N>, ClientError>
        where
            N: DeserializeOwned,
        {
            self.client.subscribe_to_method(method).await
        }
    }

    impl fmt::Debug for Client {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("Client { .. }")
        }
    }
}
