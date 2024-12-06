use std::env::current_exe;

use anyhow::Result;
use clap::Parser;

use crate::native;
#[cfg(feature = "browser-node")]
use crate::server;

#[derive(Debug, Parser)]
pub(crate) enum CliArgs {
    /// Run native node locally
    Node(native::Params),
    /// Serve compiled wasm node to be run in the browser
    #[cfg(feature = "browser-node")]
    Browser(server::Params),
}

/// Run the Lumina node.
pub async fn run() -> Result<()> {
    let _ = dotenvy::dotenv();

    let args = if current_exe()?
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        == "lumina-node"
    {
        CliArgs::Node(native::Params::parse())
    } else {
        CliArgs::parse()
    };

    let _guard = init_tracing();

    match args {
        CliArgs::Node(args) => native::run(args).await,
        #[cfg(feature = "browser-node")]
        CliArgs::Browser(args) => server::run(args).await,
    }
}

fn init_tracing() -> tracing_appender::non_blocking::WorkerGuard {
    let (non_blocking, guard) = tracing_appender::non_blocking(std::io::stdout());

    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
        .from_env_lossy();

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .init();

    guard
}
