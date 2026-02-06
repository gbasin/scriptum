use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use scriptum_daemon::rpc::{methods::RpcServerState, unix::serve_unix};
use scriptum_daemon::startup::{
    bind_socket, is_daemon_running, remove_pid_file, write_pid_file, DaemonPaths,
};

const TAKEOVER_WAIT_ATTEMPTS: usize = 20;
const TAKEOVER_WAIT_INTERVAL: Duration = Duration::from_millis(100);

pub async fn run_embedded_daemon() -> Result<()> {
    let paths = DaemonPaths::resolve().context("failed to resolve daemon runtime paths")?;

    if is_daemon_running(&paths.socket_path).await {
        takeover_standalone_daemon(&paths).await?;
    }

    write_pid_file(&paths.pid_path).context("failed to write embedded daemon pid file")?;
    let listener =
        bind_socket(&paths.socket_path).await.context("failed to bind embedded daemon socket")?;

    let serve_result = serve_unix(listener, RpcServerState::default()).await;
    remove_pid_file(&paths.pid_path);
    serve_result.context("embedded daemon rpc server exited unexpectedly")
}

async fn takeover_standalone_daemon(paths: &DaemonPaths) -> Result<()> {
    if let Some(pid) = read_pid_file(&paths.pid_path)? {
        if pid != std::process::id() {
            terminate_process(pid).with_context(|| format!("failed to terminate pid {pid}"))?;
        }
    }

    for _ in 0..TAKEOVER_WAIT_ATTEMPTS {
        if !is_daemon_running(&paths.socket_path).await {
            remove_pid_file(&paths.pid_path);
            return Ok(());
        }
        std::thread::sleep(TAKEOVER_WAIT_INTERVAL);
    }

    bail!("standalone daemon did not exit during takeover: {}", paths.socket_path.display())
}

fn read_pid_file(path: &Path) -> Result<Option<u32>> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("failed to read daemon pid file"),
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let pid = trimmed.parse::<u32>().context("daemon pid file did not contain a valid pid")?;
    Ok(Some(pid))
}

#[cfg(unix)]
fn terminate_process(pid: u32) -> Result<()> {
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .context("failed to execute kill command")?;
    if !status.success() {
        bail!("kill command returned non-zero status: {status}");
    }
    Ok(())
}

#[cfg(not(unix))]
fn terminate_process(_pid: u32) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::read_pid_file;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_path(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos();
        std::env::temp_dir().join(format!("scriptum-{name}-{nanos}.pid"))
    }

    #[test]
    fn read_pid_file_returns_none_when_missing() {
        let pid_path = unique_path("missing");
        assert_eq!(read_pid_file(&pid_path).expect("missing pid should not fail"), None);
    }

    #[test]
    fn read_pid_file_parses_integer_content() {
        let pid_path = unique_path("valid");
        fs::write(&pid_path, "12345\n").expect("pid file should be writable");
        let parsed = read_pid_file(&pid_path).expect("pid should parse");
        assert_eq!(parsed, Some(12345));
        let _ = fs::remove_file(pid_path);
    }
}
