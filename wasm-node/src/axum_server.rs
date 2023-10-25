use std::net::SocketAddr;

use anyhow::Result;
use axum::body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use clap::Parser;
use libp2p::Multiaddr;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use tokio::{spawn, time};

use celestia_node::network::Network;

const BIND_ADDR: &str = "127.0.0.1:9876";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmNodeArgs {
    pub network: Network,
    pub bootnodes: Vec<Multiaddr>,
}

#[derive(RustEmbed)]
#[folder = "pkg"]
struct WasmPackage;

#[derive(RustEmbed)]
#[folder = "rsc"]
struct StaticResources;

#[derive(Debug, Clone, Parser)]
struct Args {
    /// Network to connect.
    #[arg(short, long, value_enum, default_value_t)]
    network: Network,

    /// Bootnode multiaddr, including peer id. Can be used multiple times.
    #[arg(long)]
    bootnode: Vec<Multiaddr>,

    /// Address to serve app at
    #[arg(long, default_value = BIND_ADDR)]
    listen_addr: SocketAddr,
}

pub async fn run() -> Result<()> {
    let args = Args::parse();

    let state = WasmNodeArgs {
        network: args.network,
        bootnodes: args.bootnode,
    };

    let app = Router::new()
        .route("/", get(serve_index_html))
        .route("/js/*script", get(serve_embedded_path::<StaticResources>))
        .route("/wasm/*binary", get(serve_embedded_path::<WasmPackage>))
        .route("/cfg.json", get(serve_config))
        .with_state(state);

    spawn(axum::Server::bind(&args.listen_addr).serve(app.into_make_service()));

    loop {
        time::sleep(time::Duration::from_secs(1)).await;
    }
}

async fn serve_index_html() -> Result<impl IntoResponse, StatusCode> {
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

async fn serve_config(state: State<WasmNodeArgs>) -> Result<Response, StatusCode> {
    let args = serde_json::to_string(&state.0).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .body(body::boxed(body::Full::from(args)))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
