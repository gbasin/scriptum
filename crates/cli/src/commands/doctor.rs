// `scriptum doctor` — environment and health diagnostics.

use std::env;
use std::io::IsTerminal;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;
use url::Url;

use scriptum_common::protocol::rpc_methods;

use crate::client::DaemonClient;
use crate::output::{self, OutputFormat};

const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_RED: &str = "\x1b[31m";
const ANSI_RESET: &str = "\x1b[0m";

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Force JSON output.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DoctorStatus {
    Pass,
    Warning,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorCheck {
    name: String,
    status: DoctorStatus,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
}

impl DoctorCheck {
    fn pass(name: &str, detail: impl Into<String>) -> Self {
        Self {
            name: name.to_string(),
            status: DoctorStatus::Pass,
            detail: detail.into(),
            hint: None,
        }
    }

    fn warning(name: &str, detail: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            name: name.to_string(),
            status: DoctorStatus::Warning,
            detail: detail.into(),
            hint: Some(hint.into()),
        }
    }

    fn fail(name: &str, detail: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            name: name.to_string(),
            status: DoctorStatus::Fail,
            detail: detail.into(),
            hint: Some(hint.into()),
        }
    }
}

#[derive(Debug)]
struct DoctorPaths {
    base_dir: PathBuf,
    socket_path: PathBuf,
    ws_port_path: PathBuf,
    global_config_path: PathBuf,
}

impl DoctorPaths {
    fn detect() -> Self {
        let base_dir = scriptum_base_dir();
        Self {
            socket_path: base_dir.join("daemon.sock"),
            ws_port_path: base_dir.join("ws.port"),
            global_config_path: base_dir.join("config.toml"),
            base_dir,
        }
    }
}

#[derive(Debug, Clone)]
struct DoctorConfigContext {
    workspace_id: Option<String>,
    relay_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct GlobalConfigToml {
    relay_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct WorkspaceConfigToml {
    sync: WorkspaceSyncToml,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct WorkspaceSyncToml {
    relay_url: Option<String>,
    workspace_id: Option<String>,
    workspace_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct DocTreeResult {
    items: Vec<DocTreeItem>,
    total: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct DocTreeItem {
    doc_id: String,
    path: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DocReadHealthResult {
    degraded: bool,
}

pub fn run(args: DoctorArgs) -> anyhow::Result<()> {
    let format = OutputFormat::detect(args.json);
    let rt = if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(collect_checks()))
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build")
            .block_on(collect_checks())
    };

    match rt {
        Ok(checks) => {
            output::print_output(format, &checks, |items| format_human(items))?;
            if checks.iter().any(|check| check.status == DoctorStatus::Fail) {
                anyhow::bail!("doctor checks failed");
            }
            Ok(())
        }
        Err(error) => {
            output::print_anyhow_error(format, &error);
            Err(error)
        }
    }
}

async fn collect_checks() -> anyhow::Result<Vec<DoctorCheck>> {
    let mut checks = Vec::new();
    let paths = DoctorPaths::detect();

    checks.push(DoctorCheck::pass(
        "base_dir",
        format!("using Scriptum home at `{}`", paths.base_dir.display()),
    ));

    let daemon_check = check_daemon(&paths.socket_path).await;
    let daemon_ok = daemon_check.status == DoctorStatus::Pass;
    checks.push(daemon_check);

    checks.push(check_websocket(&paths.ws_port_path));

    let workspace_root = detect_workspace_root_from_cwd();
    checks.push(check_workspace(workspace_root.as_deref()));

    let (config_check, config_ctx) =
        check_config(&paths.global_config_path, workspace_root.as_deref());
    checks.push(config_check);

    checks.push(check_crdt_store(workspace_root.as_deref(), &config_ctx, daemon_ok).await);
    checks.push(check_git(workspace_root.as_deref()));
    checks.push(check_relay(config_ctx.relay_url.as_deref()));
    checks.push(check_mcp_binary());

    Ok(checks)
}

async fn check_daemon(socket_path: &Path) -> DoctorCheck {
    if !socket_path.exists() {
        return DoctorCheck::fail(
            "daemon",
            format!("daemon socket not found at `{}`", socket_path.display()),
            "Start daemon with: scriptumd",
        );
    }

    let client = DaemonClient::default().with_timeout(Duration::from_secs(1));
    match client.call::<_, serde_json::Value>(rpc_methods::RPC_PING, json!({})).await {
        Ok(_) => DoctorCheck::pass(
            "daemon",
            format!("daemon responded over `{}`", socket_path.display()),
        ),
        Err(error) => DoctorCheck::fail(
            "daemon",
            format!("failed to ping daemon over `{}`: {error:#}", socket_path.display()),
            "Start daemon with: scriptumd",
        ),
    }
}

fn check_websocket(ws_port_path: &Path) -> DoctorCheck {
    let raw_port = match std::fs::read_to_string(ws_port_path) {
        Ok(value) => value,
        Err(error) => {
            return DoctorCheck::fail(
                "websocket",
                format!("failed to read websocket port file `{}`: {error}", ws_port_path.display()),
                "Ensure daemon is running and has written ws.port",
            )
        }
    };

    let port: u16 = match raw_port.trim().parse() {
        Ok(port) => port,
        Err(error) => {
            return DoctorCheck::fail(
                "websocket",
                format!(
                    "invalid websocket port `{}` in `{}`: {error}",
                    raw_port.trim(),
                    ws_port_path.display()
                ),
                "Restart daemon to regenerate ws.port",
            )
        }
    };

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    match TcpStream::connect_timeout(&addr, Duration::from_millis(600)) {
        Ok(_) => DoctorCheck::pass("websocket", format!("TCP endpoint reachable at {addr}")),
        Err(error) => DoctorCheck::fail(
            "websocket",
            format!("unable to connect to websocket endpoint {addr}: {error}"),
            "Check daemon health and restart scriptumd",
        ),
    }
}

fn check_workspace(workspace_root: Option<&Path>) -> DoctorCheck {
    match workspace_root {
        Some(root) => {
            DoctorCheck::pass("workspace", format!("detected workspace at `{}`", root.display()))
        }
        None => DoctorCheck::fail(
            "workspace",
            "no `.scriptum/workspace.toml` found in current path ancestry",
            "Run: scriptum init",
        ),
    }
}

fn check_config(
    global_config_path: &Path,
    workspace_root: Option<&Path>,
) -> (DoctorCheck, DoctorConfigContext) {
    let mut details = Vec::new();
    let mut status = DoctorStatus::Pass;
    let mut hint: Option<String> = None;

    let mut relay_url = None;
    if global_config_path.exists() {
        match read_toml_file::<GlobalConfigToml>(global_config_path) {
            Ok(global) => {
                if let Some(url) = global.relay_url.clone() {
                    if let Err(error) = relay_addr_from_url(&url) {
                        status = DoctorStatus::Fail;
                        hint = Some("Fix relay_url in ~/.scriptum/config.toml".to_string());
                        details.push(format!("global relay_url is invalid: {error}"));
                    } else {
                        relay_url = Some(url);
                    }
                }
                details
                    .push(format!("global config parsed from `{}`", global_config_path.display()));
            }
            Err(error) => {
                status = DoctorStatus::Fail;
                hint = Some("Fix TOML syntax in ~/.scriptum/config.toml".to_string());
                details.push(format!("global config parse failed: {error}"));
            }
        }
    } else {
        status = worse_status(status, DoctorStatus::Warning);
        details.push("global config missing (using defaults)".to_string());
        hint = Some("Run: scriptum setup".to_string());
    }

    let mut workspace_id = None;
    if let Some(workspace_root) = workspace_root {
        let workspace_config_path = workspace_root.join(".scriptum").join("workspace.toml");
        if !workspace_config_path.exists() {
            status = DoctorStatus::Fail;
            hint = Some("Re-run: scriptum init".to_string());
            details
                .push(format!("workspace config missing at `{}`", workspace_config_path.display()));
        } else {
            match read_toml_file::<WorkspaceConfigToml>(&workspace_config_path) {
                Ok(workspace) => {
                    let id = workspace.sync.workspace_id.as_deref().map(str::trim).unwrap_or("");
                    let name =
                        workspace.sync.workspace_name.as_deref().map(str::trim).unwrap_or("");

                    if id.is_empty() || name.is_empty() {
                        status = DoctorStatus::Fail;
                        hint = Some(
                            "Run: scriptum init (or fix workspace.toml sync fields)".to_string(),
                        );
                        details.push(
                            "workspace sync.workspace_id/sync.workspace_name must be set"
                                .to_string(),
                        );
                    } else {
                        workspace_id = Some(id.to_string());
                    }

                    if workspace.sync.relay_url.is_some() {
                        relay_url = workspace.sync.relay_url.clone();
                    }

                    if let Some(url) = workspace.sync.relay_url.as_deref() {
                        if let Err(error) = relay_addr_from_url(url) {
                            status = DoctorStatus::Fail;
                            hint = Some("Fix relay_url in .scriptum/workspace.toml".to_string());
                            details.push(format!("workspace relay_url is invalid: {error}"));
                        }
                    }

                    details.push(format!(
                        "workspace config parsed from `{}`",
                        workspace_config_path.display()
                    ));
                }
                Err(error) => {
                    status = DoctorStatus::Fail;
                    hint = Some("Fix TOML syntax in .scriptum/workspace.toml".to_string());
                    details.push(format!("workspace config parse failed: {error}"));
                }
            }
        }
    } else {
        status = worse_status(status, DoctorStatus::Warning);
        details.push("workspace config validation skipped (no workspace detected)".to_string());
        if hint.is_none() {
            hint = Some("Run: scriptum init".to_string());
        }
    }

    let detail = details.join("; ");
    let check = match status {
        DoctorStatus::Pass => DoctorCheck::pass("config", detail),
        DoctorStatus::Warning => DoctorCheck::warning(
            "config",
            detail,
            hint.unwrap_or_else(|| "Review Scriptum configuration".to_string()),
        ),
        DoctorStatus::Fail => DoctorCheck::fail(
            "config",
            detail,
            hint.unwrap_or_else(|| "Fix configuration and rerun doctor".to_string()),
        ),
    };

    (check, DoctorConfigContext { workspace_id, relay_url })
}

async fn check_crdt_store(
    workspace_root: Option<&Path>,
    config_ctx: &DoctorConfigContext,
    daemon_ok: bool,
) -> DoctorCheck {
    let Some(workspace_root) = workspace_root else {
        return DoctorCheck::warning(
            "crdt_store",
            "workspace not detected; CRDT store checks skipped",
            "Run: scriptum init",
        );
    };

    let wal_dir = workspace_root.join(".scriptum").join("crdt_store").join("wal");
    let snapshots_dir = workspace_root.join(".scriptum").join("crdt_store").join("snapshots");

    if !wal_dir.is_dir() || !snapshots_dir.is_dir() {
        return DoctorCheck::fail(
            "crdt_store",
            format!(
                "missing CRDT directories (wal: `{}`, snapshots: `{}`)",
                wal_dir.display(),
                snapshots_dir.display()
            ),
            "Re-run: scriptum init",
        );
    }

    if let Err(error) = std::fs::read_dir(&wal_dir) {
        return DoctorCheck::fail(
            "crdt_store",
            format!("unable to read WAL directory `{}`: {error}", wal_dir.display()),
            "Check file permissions for .scriptum/crdt_store/wal",
        );
    }
    if let Err(error) = std::fs::read_dir(&snapshots_dir) {
        return DoctorCheck::fail(
            "crdt_store",
            format!("unable to read snapshot directory `{}`: {error}", snapshots_dir.display()),
            "Check file permissions for .scriptum/crdt_store/snapshots",
        );
    }

    if !daemon_ok {
        return DoctorCheck::warning(
            "crdt_store",
            "WAL/snapshot directories are readable; degraded-doc scan skipped (daemon unavailable)",
            "Start daemon with: scriptumd",
        );
    }

    let Some(workspace_id) = config_ctx.workspace_id.as_deref() else {
        return DoctorCheck::warning(
            "crdt_store",
            "WAL/snapshot directories are readable; degraded-doc scan skipped (missing workspace_id)",
            "Ensure `.scriptum/workspace.toml` has sync.workspace_id",
        );
    };

    let client = DaemonClient::default().with_timeout(Duration::from_secs(2));
    let tree: DocTreeResult = match client
        .call(
            rpc_methods::DOC_TREE,
            json!({
                "workspace_id": workspace_id,
            }),
        )
        .await
    {
        Ok(result) => result,
        Err(error) => {
            return DoctorCheck::fail(
                "crdt_store",
                format!("failed to query doc.tree for degraded scan: {error:#}"),
                "Verify daemon RPC health, then rerun doctor",
            )
        }
    };

    let mut degraded_paths = Vec::new();
    for item in &tree.items {
        let read_result: DocReadHealthResult = match client
            .call(
                rpc_methods::DOC_READ,
                json!({
                    "workspace_id": workspace_id,
                    "doc_id": item.doc_id,
                    "include_content": false,
                    "include_backlinks": false,
                }),
            )
            .await
        {
            Ok(result) => result,
            Err(error) => {
                return DoctorCheck::fail(
                    "crdt_store",
                    format!("failed to query doc.read for `{}`: {error:#}", item.path),
                    "Inspect daemon logs and retry",
                )
            }
        };

        if read_result.degraded {
            degraded_paths.push(item.path.clone());
        }
    }

    if degraded_paths.is_empty() {
        DoctorCheck::pass(
            "crdt_store",
            format!("WAL/snapshots readable; no degraded docs across {} document(s)", tree.total),
        )
    } else {
        DoctorCheck::fail(
            "crdt_store",
            format!("degraded docs detected: {}", degraded_paths.join(", ")),
            "Run daemon recovery and inspect WAL integrity for degraded docs",
        )
    }
}

fn check_git(workspace_root: Option<&Path>) -> DoctorCheck {
    let git_version = Command::new("git").arg("--version").output();
    match git_version {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            return DoctorCheck::fail(
                "git",
                format!("git --version exited with {}", output.status),
                "Install Git and ensure it is in PATH",
            )
        }
        Err(error) => {
            return DoctorCheck::fail(
                "git",
                format!("failed to execute git: {error}"),
                "Install Git and ensure it is in PATH",
            )
        }
    }

    let Some(workspace_root) = workspace_root else {
        return DoctorCheck::warning(
            "git",
            "git is installed; workspace remote check skipped (no workspace detected)",
            "Run: scriptum init",
        );
    };

    let inside_repo = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .output();

    let is_repo = matches!(inside_repo, Ok(output) if output.status.success());
    if !is_repo {
        return DoctorCheck::warning(
            "git",
            format!("workspace `{}` is not a git repository", workspace_root.display()),
            "Run: git init",
        );
    }

    let remotes_output = Command::new("git").arg("-C").arg(workspace_root).arg("remote").output();
    match remotes_output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let remotes: Vec<String> = stdout
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect();
            if remotes.is_empty() {
                DoctorCheck::warning(
                    "git",
                    "git is available, but no remotes are configured",
                    "Add a remote: git remote add origin <url>",
                )
            } else {
                DoctorCheck::pass("git", format!("git available; remotes: {}", remotes.join(", ")))
            }
        }
        Ok(output) => DoctorCheck::fail(
            "git",
            format!("`git remote` exited with {}", output.status),
            "Verify git repository integrity and permissions",
        ),
        Err(error) => DoctorCheck::fail(
            "git",
            format!("failed to run `git remote`: {error}"),
            "Verify git installation and repository permissions",
        ),
    }
}

fn check_relay(relay_url: Option<&str>) -> DoctorCheck {
    let Some(relay_url) = relay_url.map(str::trim).filter(|value| !value.is_empty()) else {
        return DoctorCheck::warning(
            "relay",
            "relay URL not configured (optional in local-only mode)",
            "Set relay_url in ~/.scriptum/config.toml or workspace config",
        );
    };

    let (addr, parsed_url) = match relay_addr_from_url(relay_url) {
        Ok(addr) => addr,
        Err(error) => {
            return DoctorCheck::fail(
                "relay",
                format!("invalid relay URL `{relay_url}`: {error}"),
                "Set a valid http(s) relay_url in config",
            )
        }
    };

    match TcpStream::connect_timeout(&addr, Duration::from_millis(700)) {
        Ok(_) => DoctorCheck::pass(
            "relay",
            format!("relay reachable at {}:{} ({})", addr.ip(), addr.port(), parsed_url),
        ),
        Err(error) => DoctorCheck::fail(
            "relay",
            format!("unable to reach relay at {}:{}: {error}", addr.ip(), addr.port()),
            "Check network connectivity and relay server health",
        ),
    }
}

fn check_mcp_binary() -> DoctorCheck {
    match find_binary_in_path("scriptum-mcp") {
        Some(path) => {
            DoctorCheck::pass("mcp", format!("found `scriptum-mcp` at `{}`", path.display()))
        }
        None => DoctorCheck::warning(
            "mcp",
            "`scriptum-mcp` not found in PATH",
            "Install MCP server with: npm install -g @scriptum/mcp-server",
        ),
    }
}

fn scriptum_base_dir() -> PathBuf {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".scriptum")
}

fn detect_workspace_root_from_cwd() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    find_workspace_root(&cwd)
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().find_map(|candidate| {
        let marker = candidate.join(".scriptum").join("workspace.toml");
        if marker.is_file() {
            Some(candidate.to_path_buf())
        } else {
            None
        }
    })
}

fn read_toml_file<T>(path: &Path) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    let raw = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read `{}`: {error}", path.display()))?;
    toml::from_str::<T>(&raw)
        .map_err(|error| format!("failed to parse `{}`: {error}", path.display()))
}

fn relay_addr_from_url(value: &str) -> Result<(SocketAddr, String), String> {
    let parsed = Url::parse(value).map_err(|error| format!("URL parse error: {error}"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(format!("unsupported scheme `{}` (expected http or https)", parsed.scheme()));
    }
    let host = parsed.host_str().ok_or_else(|| "relay URL must include host".to_string())?;
    let port =
        parsed.port_or_known_default().ok_or_else(|| "unable to resolve relay port".to_string())?;

    let mut addrs = (host, port)
        .to_socket_addrs()
        .map_err(|error| format!("failed to resolve relay host `{host}`: {error}"))?;
    let addr =
        addrs.next().ok_or_else(|| format!("no socket addresses resolved for relay `{host}`"))?;
    Ok((addr, parsed.to_string()))
}

fn find_binary_in_path(binary_name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    let paths = env::split_paths(&path);

    #[cfg(windows)]
    let candidates = [binary_name.to_string(), format!("{binary_name}.exe")].to_vec();
    #[cfg(not(windows))]
    let candidates = [binary_name.to_string()].to_vec();

    for dir in paths {
        for candidate in &candidates {
            let candidate_path = dir.join(candidate);
            if !candidate_path.is_file() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(metadata) = std::fs::metadata(&candidate_path) {
                    if metadata.permissions().mode() & 0o111 != 0 {
                        return Some(candidate_path);
                    }
                }
            }
            #[cfg(not(unix))]
            {
                return Some(candidate_path);
            }
        }
    }

    None
}

fn worse_status(current: DoctorStatus, next: DoctorStatus) -> DoctorStatus {
    use DoctorStatus::{Fail, Pass, Warning};
    match (current, next) {
        (Fail, _) | (_, Fail) => Fail,
        (Warning, _) | (_, Warning) => Warning,
        _ => Pass,
    }
}

fn format_human(checks: &[DoctorCheck]) -> String {
    let use_color = std::io::stdout().is_terminal();
    let mut lines = Vec::new();

    for check in checks {
        let (symbol, color) = match check.status {
            DoctorStatus::Pass => ("✓", ANSI_GREEN),
            DoctorStatus::Warning => ("⚠", ANSI_YELLOW),
            DoctorStatus::Fail => ("✗", ANSI_RED),
        };
        let rendered_symbol =
            if use_color { format!("{color}{symbol}{ANSI_RESET}") } else { symbol.to_string() };
        lines.push(format!("{rendered_symbol} {}: {}", check.name, check.detail));
        if let Some(hint) = &check.hint {
            lines.push(format!("  hint: {hint}"));
        }
    }

    let passed = checks.iter().filter(|check| check.status == DoctorStatus::Pass).count();
    let warnings = checks.iter().filter(|check| check.status == DoctorStatus::Warning).count();
    let failed = checks.iter().filter(|check| check.status == DoctorStatus::Fail).count();
    lines.push(String::new());
    lines.push(format!("Summary: {passed} passed, {warnings} warning(s), {failed} failed"));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_workspace_root_walks_upward() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let workspace_root = temp.path().join("workspace");
        let nested = workspace_root.join("docs").join("nested");
        std::fs::create_dir_all(&nested).expect("nested directories should be created");
        std::fs::create_dir_all(workspace_root.join(".scriptum"))
            .expect("workspace marker should be created");
        std::fs::write(workspace_root.join(".scriptum").join("workspace.toml"), "sync = {}")
            .expect("workspace marker should be written");

        let detected = find_workspace_root(&nested).expect("workspace root should resolve");
        assert_eq!(detected, workspace_root);
    }

    #[test]
    fn relay_addr_parser_rejects_invalid_scheme() {
        let error =
            relay_addr_from_url("ftp://relay.example.com").expect_err("ftp should be rejected");
        assert!(error.contains("unsupported scheme"));
    }

    #[test]
    fn relay_addr_parser_accepts_http_url() {
        let (addr, rendered) =
            relay_addr_from_url("http://127.0.0.1:8080").expect("http URL should parse");
        assert_eq!(addr.port(), 8080);
        assert!(rendered.starts_with("http://127.0.0.1:8080"));
    }

    #[test]
    fn format_human_includes_summary_and_hints() {
        let checks = vec![
            DoctorCheck::pass("daemon", "ok"),
            DoctorCheck::warning("relay", "not configured", "set relay_url"),
            DoctorCheck::fail("workspace", "missing", "run scriptum init"),
        ];
        let rendered = format_human(&checks);
        assert!(rendered.contains("Summary: 1 passed, 1 warning(s), 1 failed"));
        assert!(rendered.contains("hint: run scriptum init"));
    }

    #[test]
    fn worse_status_prefers_fail_then_warning() {
        assert_eq!(worse_status(DoctorStatus::Pass, DoctorStatus::Warning), DoctorStatus::Warning);
        assert_eq!(worse_status(DoctorStatus::Warning, DoctorStatus::Fail), DoctorStatus::Fail);
        assert_eq!(worse_status(DoctorStatus::Pass, DoctorStatus::Pass), DoctorStatus::Pass);
    }
}
