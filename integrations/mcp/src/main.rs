use std::path::PathBuf;

use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::EnvFilter;

mod data;
mod tools;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let fit_log_path = std::env::var("FIT_LOG_PATH").unwrap_or_else(|_| "./fit.log".to_string());
    let fit_log_path =
        std::fs::canonicalize(&fit_log_path).unwrap_or_else(|_| PathBuf::from(&fit_log_path));

    let garmin_bin = std::env::var("GARMIN_SYNC_BIN").unwrap_or_else(|_| "garmin-sync".to_string());
    let whoop_bin = std::env::var("WHOOP_SYNC_BIN").unwrap_or_else(|_| "whoop-sync".to_string());

    tracing::info!("Starting fit-mcp server");
    tracing::info!("fit.log path: {}", fit_log_path.display());

    let service = tools::FitMcp::new(fit_log_path, garmin_bin, whoop_bin)
        .serve(stdio())
        .await
        .inspect_err(|e| {
            tracing::error!("serving error: {:?}", e);
        })?;

    service.waiting().await?;
    Ok(())
}
