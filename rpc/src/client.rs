use http::{header, HeaderValue};
use jsonrpsee::http_client::{HeaderMap, HttpClient, HttpClientBuilder};

pub fn new_http(conn_str: &str, auth_token: Option<&str>) -> crate::Result<HttpClient> {
    let headers: HeaderMap<HeaderValue> = auth_token
        .into_iter()
        .map(|token| {
            HeaderValue::from_str(&format!("Bearer {token}"))
                .map(|value| (header::AUTHORIZATION, value))
        })
        .collect::<Result<_, _>>()?;

    let client = HttpClientBuilder::default()
        .set_headers(headers)
        .build(conn_str)?;

    Ok(client)
}
