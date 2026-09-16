//! Binary entrypoint: parse `--config`, build the runtime, serve, shut down.

use std::sync::Arc;

use anyhow::Context;
use second_brain_rs::{
    auth::build_authenticator,
    config::{ServerConfig, load_config},
    http::{AppState, build_router},
    mcp::SecondBrainHandler,
    observability::init_tracing,
    runtime::Runtime,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let config_path =
        parse_config_arg().context("usage: second-brain-rs --config <config.toml>")?;
    let config = load_config(&config_path).context("failed to load config")?;
    serve(config).await
}

fn parse_config_arg() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--config" {
            return args.next();
        }
    }
    None
}

async fn serve(config: ServerConfig) -> anyhow::Result<()> {
    let listen = config.listen.clone();
    let authenticator = build_authenticator(&config);
    let config = Arc::new(config);
    let runtime = Runtime::create(Arc::clone(&config))
        .await
        .context("failed to build runtime")?;
    let handler = SecondBrainHandler::new(config.as_ref(), runtime);
    let state = AppState {
        config,
        authenticator,
        handler,
    };
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .with_context(|| format!("failed to bind {listen}"))?;
    tracing::info!(%listen, "second-brain-rs listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("server error")?;
    Ok(())
}

// cancel-safe: only awaits ctrl_c; no partial state to corrupt on cancellation.
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}
