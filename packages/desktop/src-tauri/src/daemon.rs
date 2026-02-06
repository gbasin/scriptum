use anyhow::Result;

pub async fn run_embedded_daemon() -> Result<()> {
    let handle = scriptum_daemon::runtime::start_embedded().await?;
    handle.wait().await;
    Ok(())
}
