use std::net::SocketAddr;

use anyhow::Result;
use axum::body::Body;
use axum::extract::Path;
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use clap::Args;
use rust_embed::{EmbeddedFile, RustEmbed};
use tokio::net::TcpListener;
use tracing::info;

const SERVER_DEFAULT_BIND_ADDR: &str = "127.0.0.1:9876";

#[derive(RustEmbed)]
#[folder = "$WASM_NODE_OUT_DIR"]
struct WasmPackage;

#[derive(RustEmbed)]
#[folder = "../node-wasm/js"]
struct WrapperPackage;

#[derive(RustEmbed)]
#[folder = "static"]
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
        .route("/*path", get(serve_embedded_path::<StaticResources>))
        .route(
            "/lumina-node/*path",
            get(serve_embedded_path::<WrapperPackage>),
        )
        .route(
            "/lumina-node-wasm/*path",
            get(serve_embedded_path::<WasmPackage>),
        );

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
    if let Some(mut content) = Source::get(&path) {
        let mime = mime_guess::from_path(&path).first_or_octet_stream();

        if mime == mime_guess::mime::APPLICATION_JAVASCRIPT {
            // Here be dragons!
            patch_imports(&mut content);
        }

        Ok(Response::builder()
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(content.data))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// Patches imports of Javascript files to avoid duplication of them.
fn patch_imports(file: &mut EmbeddedFile) {
    file.data = String::from_utf8_lossy(&file.data)
        .replace("\"lumina-node\"", "\"/lumina-node/index.js\"")
        .replace("\"worker.js\"", "\"/lumina-node/worker.js\"")
        .replace(
            "\"lumina-node-wasm\"",
            "\"/lumina-node-wasm/lumina_node_wasm.js\"",
        )
        .into_bytes()
        .into();
}
