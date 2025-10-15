use std::net::SocketAddr;

use anyhow::Result;
use axum::body::Body;
use axum::extract::Path;
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use clap::Args;
use rust_embed::RustEmbed;
use tokio::net::TcpListener;
use tracing::info;

const SERVER_DEFAULT_BIND_ADDR: &str = "127.0.0.1:9876";

#[derive(RustEmbed)]
#[folder = "js/dist"]
struct StaticResources;

#[derive(Debug, Args)]
pub(crate) struct Params {
    /// Listening address.
    #[arg(short, long = "listen", default_value = SERVER_DEFAULT_BIND_ADDR)]
    pub(crate) listen_addr: SocketAddr,
}

pub(crate) async fn run(args: Params) -> Result<()> {
    let app = Router::new()
        .route("/", get(serve_index_html))
        .route("/{*path}", get(serve_embedded_path::<StaticResources>));

    let listener = TcpListener::bind(&args.listen_addr).await?;
    info!("Address: http://{}", args.listen_addr);

    Ok(axum::serve(listener, app.into_make_service()).await?)
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
            .body(Body::from(content.data))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}
