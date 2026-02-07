// Socket-activated startup: PID file, Unix socket creation, readiness signaling.

use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tokio::net::UnixListener;
use tracing::info;

use crate::security::{ensure_owner_only_dir, ensure_owner_only_file};

/// Default socket path: ~/.scriptum/daemon.sock
const SOCKET_NAME: &str = "daemon.sock";
/// PID file: ~/.scriptum/daemon.pid (diagnostics only)
const PID_FILE_NAME: &str = "daemon.pid";

/// Resolved paths for daemon runtime files.
pub struct DaemonPaths {
    pub base_dir: PathBuf,
    pub socket_path: PathBuf,
    pub pid_path: PathBuf,
}

impl DaemonPaths {
    /// Resolve paths under `~/.scriptum/`.
    pub fn resolve() -> Result<Self> {
        let home = dirs_path()?;
        Ok(Self {
            socket_path: home.join(SOCKET_NAME),
            pid_path: home.join(PID_FILE_NAME),
            base_dir: home,
        })
    }
}

/// Write the current process PID to `~/.scriptum/daemon.pid`.
pub fn write_pid_file(path: &Path) -> Result<()> {
    let pid = std::process::id();
    let mut file = fs::File::create(path).context("failed to create PID file")?;
    write!(file, "{pid}").context("failed to write PID")?;
    ensure_owner_only_file(path)?;
    info!(pid, path = %path.display(), "wrote PID file");
    Ok(())
}

/// Remove the PID file on shutdown.
pub fn remove_pid_file(path: &Path) {
    if let Err(e) = fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(error = %e, "failed to remove PID file");
        }
    }
}

/// Remove stale socket file and bind a new Unix listener.
/// The daemon signals readiness by accepting connections on this socket.
pub async fn bind_socket(path: &Path) -> Result<UnixListener> {
    // Remove stale socket if it exists
    if path.exists() {
        fs::remove_file(path).context("failed to remove stale socket")?;
    }

    let listener = UnixListener::bind(path).context("failed to bind Unix socket")?;
    info!(path = %path.display(), "daemon socket ready");
    Ok(listener)
}

/// Ensure the `~/.scriptum/` directory exists.
fn dirs_path() -> Result<PathBuf> {
    let home = home_dir().context("could not determine home directory")?;
    let scriptum_dir = home.join(".scriptum");
    fs::create_dir_all(&scriptum_dir).context("failed to create ~/.scriptum/")?;
    ensure_owner_only_dir(&scriptum_dir)?;
    Ok(scriptum_dir)
}

fn home_dir() -> Option<PathBuf> {
    // Prefer $HOME, fallback to platform-specific lookup
    std::env::var_os("HOME").map(PathBuf::from).or_else(|| {
        #[cfg(unix)]
        {
            // fallback: getpwuid
            None
        }
        #[cfg(not(unix))]
        {
            None
        }
    })
}

/// Check if a daemon is already running by connecting to the socket.
/// Returns true if connection succeeds (daemon is alive).
pub async fn is_daemon_running(socket_path: &Path) -> bool {
    tokio::net::UnixStream::connect(socket_path).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::TempDir;

    fn setup_test_paths(tmp: &TempDir) -> DaemonPaths {
        let base = tmp.path().to_path_buf();
        DaemonPaths {
            socket_path: base.join("daemon.sock"),
            pid_path: base.join("daemon.pid"),
            base_dir: base,
        }
    }

    #[test]
    fn test_write_and_read_pid_file() {
        let tmp = TempDir::new().unwrap();
        let paths = setup_test_paths(&tmp);

        write_pid_file(&paths.pid_path).unwrap();

        let contents = fs::read_to_string(&paths.pid_path).unwrap();
        let pid: u32 = contents.parse().unwrap();
        assert_eq!(pid, std::process::id());
    }

    #[test]
    fn test_remove_pid_file() {
        let tmp = TempDir::new().unwrap();
        let paths = setup_test_paths(&tmp);

        write_pid_file(&paths.pid_path).unwrap();
        assert!(paths.pid_path.exists());

        remove_pid_file(&paths.pid_path);
        assert!(!paths.pid_path.exists());
    }

    #[test]
    fn test_remove_nonexistent_pid_file() {
        let tmp = TempDir::new().unwrap();
        let paths = setup_test_paths(&tmp);
        // Should not panic
        remove_pid_file(&paths.pid_path);
    }

    #[tokio::test]
    async fn test_bind_socket() {
        let tmp = TempDir::new().unwrap();
        let paths = setup_test_paths(&tmp);

        let listener = bind_socket(&paths.socket_path).await.unwrap();
        assert!(paths.socket_path.exists());
        drop(listener);
    }

    #[tokio::test]
    async fn test_bind_replaces_stale_socket() {
        let tmp = TempDir::new().unwrap();
        let paths = setup_test_paths(&tmp);

        // Create first socket
        let _listener1 = bind_socket(&paths.socket_path).await.unwrap();
        drop(_listener1);

        // Should succeed even with stale socket file
        let _listener2 = bind_socket(&paths.socket_path).await.unwrap();
        assert!(paths.socket_path.exists());
    }

    #[tokio::test]
    async fn test_is_daemon_running_false() {
        let tmp = TempDir::new().unwrap();
        let sock_path = tmp.path().join("nonexistent.sock");
        assert!(!is_daemon_running(&sock_path).await);
    }

    #[tokio::test]
    async fn test_is_daemon_running_true() {
        let tmp = TempDir::new().unwrap();
        let sock_path = tmp.path().join("test.sock");

        let _listener = bind_socket(&sock_path).await.unwrap();
        assert!(is_daemon_running(&sock_path).await);
    }

    #[test]
    fn test_resolve_paths_with_custom_home() {
        let tmp = TempDir::new().unwrap();
        env::set_var("HOME", tmp.path());

        let paths = DaemonPaths::resolve().unwrap();
        assert!(paths.base_dir.ends_with(".scriptum"));
        assert!(paths.socket_path.to_string_lossy().contains("daemon.sock"));
        assert!(paths.pid_path.to_string_lossy().contains("daemon.pid"));

        // Verify directory was created
        assert!(paths.base_dir.exists());
    }
}
