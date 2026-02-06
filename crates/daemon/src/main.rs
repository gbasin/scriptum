// scriptumd: standalone mode entry point.

use anyhow::Context;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("starting standalone scriptum daemon");
    scriptum_daemon::runtime::run_standalone()
        .await
        .context("standalone daemon terminated unexpectedly")
}
