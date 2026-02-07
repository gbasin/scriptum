// Local configuration files for the daemon.
//
// Global config: `~/.scriptum/config.toml`
// Workspace config: `<workspace>/.scriptum/workspace.toml`

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::security::{ensure_owner_only_dir, ensure_owner_only_file};

/// Root directory for Scriptum global state: `~/.scriptum/`.
pub fn global_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".scriptum"))
}

/// Path to the global config file: `~/.scriptum/config.toml`.
pub fn global_config_path() -> Option<PathBuf> {
    global_dir().map(|d| d.join("config.toml"))
}

/// Path to the workspace config file: `<root>/.scriptum/workspace.toml`.
pub fn workspace_config_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".scriptum").join("workspace.toml")
}

// ── Global config ──────────────────────────────────────────────────

/// Global daemon configuration at `~/.scriptum/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GlobalConfig {
    /// Relay server URL (e.g. `https://relay.scriptum.dev`).
    pub relay_url: Option<String>,
    /// Default display name for this user.
    pub display_name: Option<String>,
    /// Default editor type (human or agent).
    pub editor_type: EditorTypeConfig,
    /// AI settings.
    pub ai: AiConfig,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            relay_url: None,
            display_name: None,
            editor_type: EditorTypeConfig::Human,
            ai: AiConfig::default(),
        }
    }
}

impl GlobalConfig {
    /// Load from `~/.scriptum/config.toml`. Returns defaults if the file
    /// doesn't exist or can't be parsed.
    pub fn load() -> Self {
        global_config_path().and_then(|p| Self::load_from(&p).ok()).unwrap_or_default()
    }

    /// Load from a specific path.
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path).map_err(ConfigError::Io)?;
        toml::from_str(&contents).map_err(ConfigError::Parse)
    }

    /// Save to `~/.scriptum/config.toml`.
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = global_config_path().ok_or_else(|| {
            ConfigError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "could not determine home directory",
            ))
        })?;
        self.save_to(&path)
    }

    /// Save to a specific path (creates parent directories).
    pub fn save_to(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(ConfigError::Io)?;
            ensure_owner_only_dir(parent)
                .map_err(|error| ConfigError::Io(std::io::Error::other(error.to_string())))?;
        }
        let contents = toml::to_string_pretty(self).map_err(ConfigError::Serialize)?;
        std::fs::write(path, contents).map_err(ConfigError::Io).and_then(|_| {
            ensure_owner_only_file(path)
                .map_err(|error| ConfigError::Io(std::io::Error::other(error.to_string())))
        })
    }
}

/// AI service configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
#[derive(Default)]
pub struct AiConfig {
    /// API keys are stored in the OS keychain, not in config files.
    /// Model to use (e.g. `claude-haiku-4-5-20251001`).
    pub model: Option<String>,
}


/// Editor type for this client.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EditorTypeConfig {
    Human,
    Agent,
}

// ── Workspace config ───────────────────────────────────────────────

/// Per-workspace configuration at `<root>/.scriptum/workspace.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[derive(Default)]
pub struct WorkspaceConfig {
    /// Git settings for this workspace.
    pub git: GitConfig,
    /// Sync settings.
    pub sync: SyncConfig,
}


impl WorkspaceConfig {
    /// Load from `<root>/.scriptum/workspace.toml`. Returns defaults if
    /// the file doesn't exist.
    pub fn load(workspace_root: &Path) -> Self {
        let path = workspace_config_path(workspace_root);
        Self::load_from(&path).unwrap_or_default()
    }

    /// Load from a specific path.
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path).map_err(ConfigError::Io)?;
        toml::from_str(&contents).map_err(ConfigError::Parse)
    }

    /// Save to `<root>/.scriptum/workspace.toml`.
    pub fn save(&self, workspace_root: &Path) -> Result<(), ConfigError> {
        let path = workspace_config_path(workspace_root);
        self.save_to(&path)
    }

    /// Save to a specific path (creates parent directories).
    pub fn save_to(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(ConfigError::Io)?;
            ensure_owner_only_dir(parent)
                .map_err(|error| ConfigError::Io(std::io::Error::other(error.to_string())))?;
        }
        let contents = toml::to_string_pretty(self).map_err(ConfigError::Serialize)?;
        std::fs::write(path, contents).map_err(ConfigError::Io).and_then(|_| {
            ensure_owner_only_file(path)
                .map_err(|error| ConfigError::Io(std::io::Error::other(error.to_string())))
        })
    }
}

/// Git sync configuration per workspace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GitConfig {
    /// Git remote name (defaults to `"origin"`).
    pub remote: String,
    /// Branch to track (defaults to `"main"`).
    pub branch: String,
    /// Auto-commit interval in seconds (0 = disabled).
    pub commit_interval_sec: u32,
    /// Push policy: `disabled`, `manual`, or `auto_rebase`.
    pub push_policy: PushPolicy,
    /// Enable AI commit message generation.
    pub ai_commit: bool,
    /// Redaction policy for AI commit messages.
    pub redaction_policy: RedactionPolicy,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            remote: "origin".into(),
            branch: "main".into(),
            commit_interval_sec: 30,
            push_policy: PushPolicy::Disabled,
            ai_commit: true,
            redaction_policy: RedactionPolicy::Redacted,
        }
    }
}

/// Push policy for git operations.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PushPolicy {
    Disabled,
    Manual,
    AutoRebase,
}

/// Redaction policy for AI commit messages.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RedactionPolicy {
    /// No AI commit messages.
    Disabled,
    /// Sanitize diffs before sending to AI.
    Redacted,
    /// Send complete diffs to AI.
    Full,
}

/// Sync configuration per workspace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[derive(Default)]
pub struct SyncConfig {
    /// Relay URL override for this workspace (uses global if unset).
    pub relay_url: Option<String>,
    /// Workspace ID on the relay server.
    pub workspace_id: Option<String>,
}


// ── Errors ─────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "config I/O error: {e}"),
            Self::Parse(e) => write!(f, "config parse error: {e}"),
            Self::Serialize(e) => write!(f, "config serialize error: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── GlobalConfig ───────────────────────────────────────────────

    #[test]
    fn global_config_defaults() {
        let cfg = GlobalConfig::default();
        assert!(cfg.relay_url.is_none());
        assert!(cfg.display_name.is_none());
        assert_eq!(cfg.editor_type, EditorTypeConfig::Human);
        assert!(cfg.ai.model.is_none());
    }

    #[test]
    fn global_config_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");

        let cfg = GlobalConfig {
            relay_url: Some("https://relay.example.com".into()),
            display_name: Some("Alice".into()),
            editor_type: EditorTypeConfig::Human,
            ai: AiConfig { model: Some("claude-haiku-4-5-20251001".into()) },
        };
        cfg.save_to(&path).unwrap();
        let loaded = GlobalConfig::load_from(&path).unwrap();
        assert_eq!(cfg, loaded);
    }

    #[test]
    fn global_config_parse_from_toml() {
        let toml_str = r#"
relay_url = "https://relay.scriptum.dev"
display_name = "Bob"
editor_type = "agent"

[ai]
model = "claude-haiku-4-5-20251001"
"#;
        let cfg: GlobalConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.relay_url.as_deref(), Some("https://relay.scriptum.dev"));
        assert_eq!(cfg.display_name.as_deref(), Some("Bob"));
        assert_eq!(cfg.editor_type, EditorTypeConfig::Agent);
        assert_eq!(cfg.ai.model.as_deref(), Some("claude-haiku-4-5-20251001"));
    }

    #[test]
    fn global_config_rejects_plaintext_api_key() {
        let toml_str = r#"
[ai]
api_key = "sk-prod"
"#;
        let error = toml::from_str::<GlobalConfig>(toml_str).expect_err("parse should fail");
        assert!(error.to_string().contains("unknown field `api_key`"));
    }

    #[test]
    fn global_config_missing_fields_use_defaults() {
        let cfg: GlobalConfig = toml::from_str("").unwrap();
        assert_eq!(cfg, GlobalConfig::default());
    }

    #[test]
    fn global_config_load_missing_file_returns_default() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("missing.toml");
        let result = GlobalConfig::load_from(&path);
        assert!(result.is_err());
    }

    // ── WorkspaceConfig ────────────────────────────────────────────

    #[test]
    fn workspace_config_defaults() {
        let cfg = WorkspaceConfig::default();
        assert_eq!(cfg.git.remote, "origin");
        assert_eq!(cfg.git.branch, "main");
        assert_eq!(cfg.git.commit_interval_sec, 30);
        assert_eq!(cfg.git.push_policy, PushPolicy::Disabled);
        assert!(cfg.git.ai_commit);
        assert_eq!(cfg.git.redaction_policy, RedactionPolicy::Redacted);
        assert!(cfg.sync.relay_url.is_none());
    }

    #[test]
    fn workspace_config_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("workspace.toml");

        let cfg = WorkspaceConfig {
            git: GitConfig {
                remote: "upstream".into(),
                branch: "develop".into(),
                commit_interval_sec: 60,
                push_policy: PushPolicy::AutoRebase,
                ai_commit: false,
                redaction_policy: RedactionPolicy::Full,
            },
            sync: SyncConfig {
                relay_url: Some("https://custom-relay.example.com".into()),
                workspace_id: Some("ws-123".into()),
            },
        };
        cfg.save_to(&path).unwrap();
        let loaded = WorkspaceConfig::load_from(&path).unwrap();
        assert_eq!(cfg, loaded);
    }

    #[test]
    fn workspace_config_parse_from_toml() {
        let toml_str = r#"
[git]
remote = "origin"
branch = "main"
commit_interval_sec = 30
push_policy = "manual"
ai_commit = true
redaction_policy = "disabled"

[sync]
relay_url = "https://relay.example.com"
workspace_id = "ws-456"
"#;
        let cfg: WorkspaceConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.git.push_policy, PushPolicy::Manual);
        assert_eq!(cfg.git.redaction_policy, RedactionPolicy::Disabled);
        assert_eq!(cfg.sync.workspace_id.as_deref(), Some("ws-456"));
    }

    #[test]
    fn workspace_config_partial_toml_uses_defaults() {
        let toml_str = r#"
[git]
branch = "develop"
"#;
        let cfg: WorkspaceConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.git.branch, "develop");
        assert_eq!(cfg.git.remote, "origin"); // default
        assert_eq!(cfg.git.push_policy, PushPolicy::Disabled); // default
    }

    #[test]
    fn workspace_config_load_from_workspace_root() {
        let dir = TempDir::new().unwrap();
        let ws_root = dir.path().join("my-project");
        std::fs::create_dir_all(&ws_root).unwrap();

        let cfg = WorkspaceConfig::default();
        cfg.save(&ws_root).unwrap();

        let loaded = WorkspaceConfig::load(&ws_root);
        assert_eq!(cfg, loaded);

        // Verify the file path.
        let expected_path = ws_root.join(".scriptum").join("workspace.toml");
        assert!(expected_path.exists());
    }

    #[test]
    fn workspace_config_load_missing_returns_default() {
        let dir = TempDir::new().unwrap();
        let loaded = WorkspaceConfig::load(dir.path());
        assert_eq!(loaded, WorkspaceConfig::default());
    }

    // ── Path helpers ───────────────────────────────────────────────

    #[test]
    fn global_dir_is_under_home() {
        let dir = global_dir();
        assert!(dir.is_some());
        let dir = dir.unwrap();
        assert!(dir.ends_with(".scriptum"));
    }

    #[test]
    fn workspace_config_path_is_correct() {
        let root = PathBuf::from("/projects/my-project");
        let path = workspace_config_path(&root);
        assert_eq!(path, PathBuf::from("/projects/my-project/.scriptum/workspace.toml"));
    }

    // ── Serialization format ───────────────────────────────────────

    #[test]
    fn push_policy_serializes_correctly() {
        assert_eq!(serde_json::to_string(&PushPolicy::AutoRebase).unwrap(), "\"auto_rebase\"");
        assert_eq!(serde_json::to_string(&PushPolicy::Manual).unwrap(), "\"manual\"");
    }

    #[test]
    fn redaction_policy_serializes_correctly() {
        assert_eq!(serde_json::to_string(&RedactionPolicy::Redacted).unwrap(), "\"redacted\"");
    }

    #[test]
    fn global_config_creates_parent_directories() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("deep").join("nested").join("config.toml");

        let cfg = GlobalConfig::default();
        cfg.save_to(&path).unwrap();
        assert!(path.exists());
    }
}
