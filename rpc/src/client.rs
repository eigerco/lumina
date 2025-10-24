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

    use http::{HeaderValue, header};
    use jsonrpsee::core::ClientError;
    use jsonrpsee::core::client::{BatchResponse, ClientT, Subscription, SubscriptionClientT};
    use jsonrpsee::core::params::BatchRequestBuilder;
    use jsonrpsee::core::traits::ToRpcParams;
    use jsonrpsee::http_client::{HeaderMap, HttpClient, HttpClientBuilder};
    use jsonrpsee::ws_client::{PingConfig, WsClient, WsClientBuilder};
    use serde::de::DeserializeOwned;

    use crate::Error;

    const MAX_RESPONSE_SIZE: usize = 256 * 1024 * 1024;

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
                        .enable_ws_ping(PingConfig::default())
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
    use std::sync::atomic::{AtomicU64, Ordering};

    use gloo_net::http::{Request as JsRequest, Response as JsResponse};
    use jsonrpsee::core::client::{BatchResponse, ClientT, Subscription, SubscriptionClientT};
    use jsonrpsee::core::middleware::Batch;
    use jsonrpsee::core::params::BatchRequestBuilder;
    use jsonrpsee::core::traits::ToRpcParams;
    use jsonrpsee::core::{ClientError, JsonRawValue};
    use jsonrpsee::types::{
        Id, InvalidRequestId, Notification, Request, Response, ResponseSuccess,
    };
    use send_wrapper::SendWrapper;
    use serde::Serialize;
    use serde::de::DeserializeOwned;

    use crate::Error;

    /// Json RPC client.
    pub struct Client {
        id: AtomicU64,
        url: String,
        auth_token: Option<String>,
    }

    impl Client {
        /// Create a new Json RPC client.
        ///
        /// Only the 'http\[s\]' protocols are supported because javascirpt
        /// doesn't allow setting headers with websocket. If you want to
        /// use the websocket client anyway, you can use the one from the
        /// `jsonrpsee` directly, but you need a node with `--rpc.skip-auth`.
        pub async fn new(conn_str: &str, auth_token: Option<&str>) -> Result<Self, Error> {
            let protocol = conn_str.split_once(':').map(|(proto, _)| proto);
            match protocol {
                Some("http") | Some("https") => (),
                _ => return Err(Error::ProtocolNotSupported(conn_str.into())),
            };

            Ok(Client {
                id: AtomicU64::new(0),
                url: conn_str.into(),
                auth_token: auth_token.map(ToOwned::to_owned),
            })
        }
    }

    impl Client {
        async fn send<T: Serialize>(&self, request: T) -> Result<JsResponse, ClientError> {
            let fut = {
                let mut req = JsRequest::post(&self.url);

                if let Some(token) = self.auth_token.as_ref() {
                    req = req.header("Authorization", &format!("Bearer {token}"));
                }

                req.json(&request).map_err(into_parse_error)?.send()
            };

            SendWrapper::new(fut)
                .await
                .map_err(|e| ClientError::Transport(e.into()))
        }

        async fn send_and_read_body<T: Serialize>(
            &self,
            request: T,
        ) -> Result<Vec<u8>, ClientError> {
            let response = SendWrapper::new(self.send(request).await?);

            if !response.ok() {
                let error = format!(
                    "Request failed: {} {}",
                    response.status(),
                    response.status_text(),
                );
                return Err(ClientError::Transport(error.into()));
            }

            SendWrapper::new(response.binary())
                .await
                .map_err(|err| ClientError::Transport(err.into()))
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
            let params = params.to_rpc_params()?;
            let notification = Notification::new(method.into(), params);

            self.send(notification).await?;

            Ok(())
        }

        async fn request<R, Params>(&self, method: &str, params: Params) -> Result<R, ClientError>
        where
            R: DeserializeOwned,
            Params: ToRpcParams + Send,
        {
            let id = Id::Number(self.id.fetch_add(1, Ordering::Relaxed));
            let params = params.to_rpc_params()?;
            let request = Request::borrowed(method, params.as_deref(), id.clone());

            let body = self.send_and_read_body(request).await?;
            let response: Response<&JsonRawValue> = serde_json::from_slice(&body)?;

            let success = ResponseSuccess::try_from(response)?;

            if success.id == id {
                let result = serde_json::from_str(success.result.get())?;
                Ok(result)
            } else {
                Err(InvalidRequestId::NotPendingRequest(success.id.to_string()).into())
            }
        }

        async fn batch_request<'a, R>(
            &self,
            batch: BatchRequestBuilder<'a>,
        ) -> Result<BatchResponse<'a, R>, ClientError>
        where
            R: DeserializeOwned + fmt::Debug + 'a,
        {
            // this will throw an error if batch is empty
            let batch = batch.build()?;
            let batch_len = batch.len();

            // fill in batch request
            let mut ids = Vec::with_capacity(batch_len);
            let mut batch_request = Batch::with_capacity(batch_len);

            for (method, params) in batch.into_iter() {
                let id = self.id.fetch_add(1, Ordering::Relaxed);
                let request = Request::owned(method.into(), params, Id::Number(id));

                ids.push(id);
                batch_request.push(request);
            }

            // send it and grab response
            let body = self.send_and_read_body(batch_request).await?;
            let mut resps: Vec<Response<&JsonRawValue>> = serde_json::from_slice(&body)?;

            // no docs mention that responses should be returned in order
            // but jsonrpsee implementations always take care to do that
            resps.sort_by(|lhs, rhs| {
                // put those with non-numeric ids first, we'll error out on them
                // at the beginning of the next loop
                let lhs_id = lhs.id.try_parse_inner_as_number().unwrap_or(0);
                let rhs_id = rhs.id.try_parse_inner_as_number().unwrap_or(0);
                lhs_id.cmp(&rhs_id)
            });

            // prepare the batch result
            let mut successful = 0;
            let mut failed = 0;

            let mut batch_resp = Vec::with_capacity(batch_len);
            for resp in resps.into_iter() {
                let id = resp.id.try_parse_inner_as_number()?;
                if !ids.contains(&id) {
                    return Err(InvalidRequestId::NotPendingRequest(id.to_string()).into());
                }

                let res = match ResponseSuccess::try_from(resp) {
                    Ok(success) => {
                        successful += 1;
                        Ok(serde_json::from_str(success.result.get())?)
                    }
                    Err(error) => {
                        failed += 1;
                        Err(error)
                    }
                };

                batch_resp.push(res);
            }

            Ok(BatchResponse::new(successful, batch_resp, failed))
        }
    }

    impl SubscriptionClientT for Client {
        async fn subscribe<'a, N, Params>(
            &self,
            _subscribe_method: &'a str,
            _params: Params,
            _unsubscribe_method: &'a str,
        ) -> Result<Subscription<N>, ClientError>
        where
            Params: ToRpcParams + Send,
            N: DeserializeOwned,
        {
            Err(ClientError::HttpNotImplemented)
        }

        async fn subscribe_to_method<N>(
            &self,
            _method: &str,
        ) -> Result<Subscription<N>, ClientError>
        where
            N: DeserializeOwned,
        {
            Err(ClientError::HttpNotImplemented)
        }
    }

    impl fmt::Debug for Client {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("Client { .. }")
        }
    }

    /// Used to translate gloo errors originating from serde into client errors.
    /// Can panic if used in a place that has other gloo error types possible.
    fn into_parse_error(err: gloo_net::Error) -> ClientError {
        match err {
            gloo_net::Error::SerdeError(e) => ClientError::ParseError(e),
            _ => unreachable!("this can only fail on a call to serde_json"),
        }
    }
}
