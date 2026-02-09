use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub async fn run_embedded_daemon() -> Result<()> {
    if let Ok(socket_path) = daemon_socket_path() {
        if socket_is_responsive(&socket_path).await {
            eprintln!(
                "standalone daemon detected at `{}`; requesting takeover before embedded startup",
                socket_path.display()
            );
        }
    }

    let handle = scriptum_daemon::runtime::start_embedded().await?;
    handle.wait().await;
    Ok(())
}

fn daemon_socket_path() -> Result<PathBuf> {
    let paths = scriptum_daemon::startup::DaemonPaths::resolve()
        .context("failed to resolve daemon runtime paths")?;
    Ok(paths.socket_path)
}

async fn socket_is_responsive(socket_path: &Path) -> bool {
    scriptum_daemon::startup::is_daemon_running(socket_path).await
}

#[cfg(all(test, unix))]
mod tests {
    use super::socket_is_responsive;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::net::UnixListener;

    fn temp_socket_path(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("scriptum-desktop-{label}-{}-{nanos}.sock", std::process::id()))
    }

    #[tokio::test]
    async fn socket_probe_returns_false_for_missing_socket() {
        let socket_path = temp_socket_path("missing");
        assert!(!socket_is_responsive(&socket_path).await);
    }

    #[tokio::test]
    async fn socket_probe_returns_true_for_live_socket() {
        let socket_path = temp_socket_path("live");
        let listener = UnixListener::bind(&socket_path).expect("unix listener should bind");
        assert!(socket_is_responsive(&socket_path).await);
        drop(listener);
        let _ = std::fs::remove_file(&socket_path);
    }
}
