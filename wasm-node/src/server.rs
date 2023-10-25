use std::net::SocketAddr;

use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::{body, Json, Router};
use clap::{Parser, ValueEnum};
use libp2p::Multiaddr;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

const BIND_ADDR: &str = "127.0.0.1:9876";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmNodeArgs {
    pub network: ArgNetwork,
    pub bootnodes: Vec<Multiaddr>,
}

#[derive(RustEmbed)]
#[folder = "pkg"]
struct WasmPackage;

#[derive(RustEmbed)]
#[folder = "static"]
struct StaticResources;

#[derive(Debug, Clone, Parser)]
struct Args {
    /// Network to connect.
    #[arg(short, long, value_enum, default_value_t)]
    network: ArgNetwork,

    /// Bootnode multiaddr, including peer id. Can be used multiple times.
    #[arg(long)]
    bootnode: Vec<Multiaddr>,

    /// Address to serve app at
    #[arg(long, default_value = BIND_ADDR)]
    listen_addr: SocketAddr,
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize_repr, Deserialize_repr,
)]
#[repr(u8)]
pub enum ArgNetwork {
    Arabica,
    Mocha,
    #[default]
    Private,
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

    Ok(axum::Server::bind(&args.listen_addr)
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
