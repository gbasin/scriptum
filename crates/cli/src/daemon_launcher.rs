use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};

#[cfg(unix)]
use tokio::net::UnixStream;
#[cfg(unix)]
use tokio::time::sleep;

const SOCKET_RELATIVE_PATH: &str = ".scriptum/daemon.sock";
const CONNECT_RETRIES: usize = 20;
const RETRY_DELAY_MS: u64 = 100;

pub async fn ensure_daemon_running() -> Result<()> {
    #[cfg(unix)]
    {
        let socket_path = default_socket_path();

        match try_connect(&socket_path).await {
            Ok(()) => return Ok(()),
            Err(err) if should_attempt_launch(err.kind()) => {
                spawn_daemon_process()?;
                wait_for_daemon_socket(&socket_path).await?;
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("failed to connect to daemon socket at `{}`", socket_path.display())
                });
            }
        }
    }

    #[cfg(not(unix))]
    {
        // TODO: Add named pipe launcher support for Windows.
    }

    Ok(())
}

fn default_socket_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    socket_path_from_home(&home)
}

fn socket_path_from_home(home: &Path) -> PathBuf {
    home.join(SOCKET_RELATIVE_PATH)
}

fn should_attempt_launch(kind: io::ErrorKind) -> bool {
    matches!(kind, io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound)
}

fn daemon_candidates() -> Vec<OsString> {
    if let Some(explicit_binary) = std::env::var_os("SCRIPTUM_DAEMON_BIN") {
        vec![explicit_binary]
    } else {
        vec![OsString::from("scriptumd"), OsString::from("scriptum-daemon")]
    }
}

fn spawn_daemon_process() -> Result<()> {
    let mut not_found_candidates = Vec::new();

    for candidate in daemon_candidates() {
        let mut command = Command::new(&candidate);
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());

        match command.spawn() {
            Ok(_child) => return Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                not_found_candidates.push(candidate);
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("failed to spawn daemon process with binary `{:?}`", candidate)
                });
            }
        }
    }

    Err(anyhow!(
        "unable to find daemon binary (tried: {})",
        not_found_candidates
            .iter()
            .map(|name| name.to_string_lossy())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

#[cfg(unix)]
async fn try_connect(socket_path: &Path) -> io::Result<()> {
    UnixStream::connect(socket_path).await.map(|_| ())
}

#[cfg(unix)]
async fn wait_for_daemon_socket(socket_path: &Path) -> Result<()> {
    let mut last_error: Option<io::Error> = None;

    for _ in 0..CONNECT_RETRIES {
        match try_connect(socket_path).await {
            Ok(()) => return Ok(()),
            Err(err) if should_attempt_launch(err.kind()) => {
                last_error = Some(err);
                sleep(Duration::from_millis(RETRY_DELAY_MS)).await;
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "daemon started but socket connection failed at `{}`",
                        socket_path.display()
                    )
                });
            }
        }
    }

    Err(anyhow!(
        "daemon socket did not become available at `{}` after {} retries; last error: {}",
        socket_path.display(),
        CONNECT_RETRIES,
        last_error.map(|err| err.to_string()).unwrap_or_else(|| "unknown".to_string())
    ))
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;
    use std::path::PathBuf;

    use super::{should_attempt_launch, socket_path_from_home};

    #[test]
    fn launches_when_socket_missing_or_refused() {
        assert!(should_attempt_launch(ErrorKind::NotFound));
        assert!(should_attempt_launch(ErrorKind::ConnectionRefused));
        assert!(!should_attempt_launch(ErrorKind::PermissionDenied));
    }

    #[test]
    fn derives_socket_path_from_home_directory() {
        let home = PathBuf::from("/tmp/scriptum-home");
        let socket_path = socket_path_from_home(&home);

        assert_eq!(socket_path, PathBuf::from("/tmp/scriptum-home/.scriptum/daemon.sock"));
    }
}
