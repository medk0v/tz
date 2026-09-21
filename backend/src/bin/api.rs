use anyhow::{Context, Result};
use tokio::net::TcpListener;
use tracing::info;
use tz_backend::{
    AppState, Config,
    cli::{self, Command},
    router, telemetry,
};

#[tokio::main]
async fn main() -> Result<()> {
    let Command::Run { config_path } = cli::parse()? else {
        cli::print_usage("api");
        return Ok(());
    };
    let config = Config::from_file(config_path)?;
    let _telemetry = telemetry::init(&config.logging, "api")?;

    let address = config.server.address();
    let state = AppState::build(config).await?;
    tz_backend::realtime::spawn_redis_subscriber(state.clone());

    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("failed to bind API to {address}"))?;
    info!(%address, "API listening");

    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("API server failed")
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install termination signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
