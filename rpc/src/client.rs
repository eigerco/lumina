use http::{header, HeaderValue};
use jsonrpsee::http_client::{HeaderMap, HttpClient, HttpClientBuilder};
use jsonrpsee::ws_client::{WsClient, WsClientBuilder};

use crate::Result;

pub fn new_http(conn_str: &str, auth_token: Option<&str>) -> Result<HttpClient> {
    let mut headers = HeaderMap::new();

    if let Some(token) = auth_token {
        let val = HeaderValue::from_str(&format!("Bearer {token}"))?;
        headers.insert(header::AUTHORIZATION, val);
    }

    let client = HttpClientBuilder::default()
        .set_headers(headers)
        .build(conn_str)?;

    Ok(client)
}

pub async fn new_websocket(conn_str: &str, auth_token: Option<&str>) -> Result<WsClient> {
    let mut headers = HeaderMap::new();

    if let Some(token) = auth_token {
        let val = HeaderValue::from_str(&format!("Bearer {token}"))?;
        headers.insert(header::AUTHORIZATION, val);
    }

    let client = WsClientBuilder::default()
        .set_headers(headers)
        .build(conn_str)
        .await?;

    Ok(client)
}
