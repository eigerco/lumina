use std::net::SocketAddr;

use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::{body, Json, Router};
use libp2p::Multiaddr;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::common::ArgNetwork;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WasmNodeArgs {
    pub network: ArgNetwork,
    pub bootnodes: Vec<Multiaddr>,
}

#[derive(RustEmbed)]
#[folder = "../wasm-node/pkg"]
struct WasmPackage;

#[derive(RustEmbed)]
#[folder = "static"]
struct StaticResources;

pub(crate) async fn run(
    network: ArgNetwork,
    bootnodes: Vec<Multiaddr>,
    listen_addr: SocketAddr,
) -> Result<()> {
    let state = WasmNodeArgs { network, bootnodes };

    let app = Router::new()
        .route("/", get(serve_index_html))
        .route("/js/*path", get(serve_embedded_path::<StaticResources>))
        .route("/wasm/*path", get(serve_embedded_path::<WasmPackage>))
        .route("/cfg.json", get(serve_config))
        .with_state(state);

    info!("listening on {listen_addr}");
    Ok(axum::Server::bind(&listen_addr)
        .serve(app.into_make_service())
        .await?)
}

async fn serve_index_html() -> Result<Response, StatusCode> {
    serve_embedded_path::<StaticResources>(Path("index.html".to_string())).await
}

async fn serve_embedded_path<Source: RustEmbed>(
    Path(path): Path<String>,
) -> Result<Response, StatusCode> {
    if let Some(content) = Source::get(&path) {
        let mime = mime_guess::from_path(&path).first_or_octet_stream();
        Ok(Response::builder()
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(body::boxed(body::Full::from(content.data)))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn serve_config(state: State<WasmNodeArgs>) -> Json<WasmNodeArgs> {
    Json(state.0)
}
