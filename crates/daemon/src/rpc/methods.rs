use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, UNIX_EPOCH};

use crate::agent::lease::{LeaseClaim, LeaseMode, LeaseStore};
use crate::agent::session::{AgentSession as PersistedAgentSession, SessionStatus, SessionStore};
use crate::config::{
    workspace_config_path, GlobalConfig, RedactionPolicy as ConfigRedactionPolicy, WorkspaceConfig,
};
use crate::engine::{doc_manager::DocManager, ydoc::YDoc};
use crate::git::commit::{
    fallback_commit_message, generate_commit_message_with_fallback, AiCommitClient,
    AnthropicCommitClient, RedactionPolicy as AiRedactionPolicy,
};
use crate::git::triggers::{
    ChangeType, ChangedFile, TriggerCollector, TriggerConfig, TriggerEvent,
};
use crate::git::worker::{CommandExecutor, GitWorker, ProcessCommandExecutor};
use crate::rpc::trace::{trace_id_from_raw_request, with_trace_id_scope};
use crate::search::indexer::extract_title;
use crate::search::{
    resolve_wiki_links, BacklinkStore, Fts5Index, IndexEntry, LinkableDocument, SearchHit,
    SearchIndex,
};
use crate::store::documents_local::{DocumentsLocalStore, LocalDocumentRecord};
use crate::store::meta_db::MetaDb;
use crate::store::recovery::{recover_documents_into_manager, StartupRecoveryReport};
use crate::store::wal::WalStore;
use crate::watcher::hash::sha256_hex;
use base64::Engine;
use regex::Regex;
use scriptum_common::backlink::parse_wiki_links;
use scriptum_common::path::normalize_path;
use scriptum_common::protocol::jsonrpc::{
    is_supported_protocol_version, Request, RequestId, Response, RpcError,
    CURRENT_PROTOCOL_VERSION as RPC_CURRENT_PROTOCOL_VERSION, INTERNAL_ERROR, INVALID_PARAMS,
    INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR,
    SUPPORTED_PROTOCOL_VERSIONS as RPC_SUPPORTED_PROTOCOL_VERSIONS,
};
use scriptum_common::protocol::rpc_methods;
use scriptum_common::section::{parser::parse_sections, slug::slugify};
use scriptum_common::types::{
    AgentSession as RpcAgentSession, Document as RpcDocument, EditorType, OverlapEditor,
    OverlapSeverity, Section, SectionOverlap, Workspace as RpcWorkspace,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tiktoken_rs::CoreBPE;
use tokio::sync::broadcast;
use tokio::sync::RwLock;
use tracing::{info_span, warn, Instrument};
use uuid::Uuid;

// ── Git sync policy ─────────────────────────────────────────────────

/// Controls git sync behavior for this workspace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum GitSyncPolicy {
    /// No automatic git operations.
    Disabled,
    /// Commit on triggers but never push.
    #[default]
    Manual,
    /// Commit + push with rebase on triggers.
    AutoRebase,
}

// ── Git state ───────────────────────────────────────────────────────

/// Git-related state for the RPC server.
#[derive(Clone)]
pub struct GitState<E: CommandExecutor = ProcessCommandExecutor> {
    worker: Arc<GitWorker<E>>,
    policy: Arc<RwLock<GitSyncPolicy>>,
    last_sync_at: Arc<RwLock<Option<chrono::DateTime<chrono::Utc>>>>,
    ai_client: Arc<dyn AiCommitClient>,
    ai_enabled: bool,
    ai_configured: bool,
    ai_redaction_policy: AiRedactionPolicy,
}

impl GitState<ProcessCommandExecutor> {
    pub fn new(repo_path: impl Into<PathBuf>) -> Self {
        Self::with_executor(repo_path, ProcessCommandExecutor)
    }
}

impl<E: CommandExecutor> GitState<E> {
    pub fn with_executor(repo_path: impl Into<PathBuf>, executor: E) -> Self {
        let repo_path = repo_path.into();
        let workspace_config = WorkspaceConfig::load(&repo_path);
        let global_config = GlobalConfig::load();
        let anthropic_client = Arc::new(AnthropicCommitClient::from_global_config(&global_config));
        let ai_enabled = workspace_config.git.ai_commit && global_config.ai.enabled;

        Self::with_executor_and_ai_config(
            repo_path,
            executor,
            anthropic_client.clone(),
            ai_enabled,
            anthropic_client.is_configured(),
            map_workspace_redaction_policy(workspace_config.git.redaction_policy),
        )
    }

    pub fn with_executor_and_ai(
        repo_path: impl Into<PathBuf>,
        executor: E,
        ai_client: Arc<dyn AiCommitClient>,
        ai_enabled: bool,
        ai_redaction_policy: AiRedactionPolicy,
    ) -> Self {
        Self::with_executor_and_ai_config(
            repo_path,
            executor,
            ai_client,
            ai_enabled,
            ai_enabled,
            ai_redaction_policy,
        )
    }

    fn with_executor_and_ai_config(
        repo_path: impl Into<PathBuf>,
        executor: E,
        ai_client: Arc<dyn AiCommitClient>,
        ai_enabled: bool,
        ai_configured: bool,
        ai_redaction_policy: AiRedactionPolicy,
    ) -> Self {
        let repo_path = repo_path.into();
        Self {
            worker: Arc::new(GitWorker::with_executor(repo_path, executor)),
            policy: Arc::new(RwLock::new(GitSyncPolicy::default())),
            last_sync_at: Arc::new(RwLock::new(None)),
            ai_client,
            ai_enabled,
            ai_configured,
            ai_redaction_policy,
        }
    }

    async fn commit_with_generated_message(
        &self,
        semantic_hint: &str,
        trigger_type: Option<&str>,
    ) -> Result<(), String> {
        let _ = self.worker.add(&["."]).map_err(|e| e.to_string())?;

        let staged_diff = self.worker.diff_cached().map_err(|e| e.to_string())?;
        let staged_name_status =
            self.worker.diff_cached_name_status().map_err(|e| e.to_string())?;
        let changed_files = parse_changed_files_from_name_status(&staged_name_status.stdout);

        let commit_message = if self.ai_enabled && self.ai_configured {
            let mut prompt = String::new();
            let trimmed_hint = semantic_hint.trim();
            if !trimmed_hint.is_empty() {
                prompt.push_str("Trigger summary:\n");
                prompt.push_str(trimmed_hint);
                prompt.push_str("\n\n");
            }
            prompt.push_str("Staged diff:\n");
            prompt.push_str(&staged_diff.stdout);

            generate_commit_message_with_fallback(
                self.ai_client.as_ref(),
                &prompt,
                &changed_files,
                self.ai_redaction_policy,
            )
            .await
        } else {
            fallback_commit_message(&changed_files)
        };

        let commit_message = append_trigger_metadata(commit_message, trigger_type);
        self.worker.commit(&commit_message).map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn map_workspace_redaction_policy(value: ConfigRedactionPolicy) -> AiRedactionPolicy {
    match value {
        ConfigRedactionPolicy::Disabled => AiRedactionPolicy::Disabled,
        ConfigRedactionPolicy::Redacted => AiRedactionPolicy::Redacted,
        ConfigRedactionPolicy::Full => AiRedactionPolicy::Full,
    }
}

fn parse_changed_files_from_name_status(output: &str) -> Vec<ChangedFile> {
    output
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }

            let mut parts = trimmed.split_whitespace();
            let status = parts.next()?;
            let path = parts.next_back().or_else(|| parts.next())?;

            Some(ChangedFile {
                path: path.to_string(),
                doc_id: None,
                change_type: parse_change_type(status),
            })
        })
        .collect()
}

fn parse_change_type(status: &str) -> ChangeType {
    match status.chars().next() {
        Some('A') => ChangeType::Added,
        Some('D') => ChangeType::Deleted,
        _ => ChangeType::Modified,
    }
}

fn append_trigger_metadata(message: String, trigger_type: Option<&str>) -> String {
    let Some(trigger_type) = trigger_type.map(str::trim).filter(|value| !value.is_empty()) else {
        return message;
    };

    let mut composed = message.trim_end().to_string();
    composed.push_str("\n\nScriptum-Trigger: ");
    composed.push_str(trigger_type);
    composed
}

const HISTORY_SYSTEM_AUTHOR_ID: &str = "system";
const HISTORY_LOCAL_HUMAN_AUTHOR_ID: &str = "local-user";

#[derive(Debug, Clone)]
struct DocSnapshotRecord {
    content_md: String,
    timestamp: chrono::DateTime<chrono::Utc>,
    author_id: String,
    author_type: EditorType,
    summary: Option<String>,
}

#[derive(Clone)]
pub struct RpcServerState {
    doc_manager: Arc<RwLock<DocManager>>,
    doc_metadata: Arc<RwLock<HashMap<(Uuid, Uuid), DocMetadataRecord>>>,
    doc_history: Arc<RwLock<HashMap<(Uuid, Uuid), BTreeMap<i64, DocSnapshotRecord>>>>,
    degraded_docs: Arc<RwLock<HashSet<Uuid>>>,
    crdt_store_dir: Arc<PathBuf>,
    global_config_path: Option<PathBuf>,
    workspaces: Arc<RwLock<HashMap<Uuid, WorkspaceInfo>>>,
    shutdown_notifier: Option<broadcast::Sender<()>>,
    git_state: Option<Arc<dyn GitOps + Send + Sync>>,
    git_triggers: Arc<Mutex<TriggerCollector>>,
    git_idle_timer_epoch: Arc<AtomicU64>,
    agent_db: Arc<Mutex<MetaDb>>,
    lease_store: Arc<Mutex<LeaseStore>>,
    agent_id: Arc<String>,
}

/// Trait to abstract git operations for testability via dynamic dispatch.
trait GitOps: Send + Sync {
    fn status_info(&self) -> Result<GitStatusInfo, String>;
    fn sync(
        &self,
        action: GitSyncAction,
    ) -> Pin<Box<dyn Future<Output = Result<Uuid, String>> + Send + '_>>;
    fn get_policy(&self) -> GitSyncPolicy;
    fn set_policy(&self, policy: GitSyncPolicy);
    fn last_sync_at(&self) -> Option<chrono::DateTime<chrono::Utc>>;
    fn mark_synced(&self);
}

impl<E: CommandExecutor + 'static> GitOps for GitState<E> {
    fn status_info(&self) -> Result<GitStatusInfo, String> {
        let output = self.worker.status().map_err(|e| e.to_string())?;
        let dirty = !output.stdout.trim().is_empty();
        // Parse branch from `git status --short` — it doesn't include branch info.
        // We'll return the raw status output and dirty flag.
        Ok(GitStatusInfo {
            dirty,
            status_output: output.stdout,
            policy: {
                // Can't await in a sync function — use try_read.
                self.policy.try_read().map(|p| p.clone()).unwrap_or_default()
            },
            last_sync_at: self.last_sync_at.try_read().ok().and_then(|v| *v),
            ai_configured: self.ai_enabled && self.ai_configured,
        })
    }

    fn sync(
        &self,
        action: GitSyncAction,
    ) -> Pin<Box<dyn Future<Output = Result<Uuid, String>> + Send + '_>> {
        Box::pin(async move {
            let job_id = Uuid::new_v4();

            match action {
                GitSyncAction::Commit { message, trigger_type } => {
                    self.commit_with_generated_message(&message, trigger_type.as_deref()).await?;
                }
                GitSyncAction::CommitAndPush { message, trigger_type } => {
                    self.commit_with_generated_message(&message, trigger_type.as_deref()).await?;
                    self.worker.push().map_err(|e| e.to_string())?;
                }
            }

            self.mark_synced();
            Ok(job_id)
        })
    }

    fn get_policy(&self) -> GitSyncPolicy {
        self.policy.try_read().map(|p| p.clone()).unwrap_or_default()
    }

    fn set_policy(&self, policy: GitSyncPolicy) {
        if let Ok(mut guard) = self.policy.try_write() {
            *guard = policy;
        }
    }

    fn last_sync_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.last_sync_at.try_read().ok().and_then(|v| *v)
    }

    fn mark_synced(&self) {
        if let Ok(mut guard) = self.last_sync_at.try_write() {
            *guard = Some(chrono::Utc::now());
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocMetadataRecord {
    pub workspace_id: Uuid,
    pub doc_id: Uuid,
    pub path: String,
    pub title: String,
    pub head_seq: i64,
    pub etag: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DocReadParams {
    workspace_id: Uuid,
    doc_id: Uuid,
    #[serde(default)]
    include_content: bool,
    #[serde(default)]
    include_backlinks: bool,
}

#[derive(Debug, Clone, Serialize)]
struct DocReadResult {
    document: RpcDocument,
    sections: Vec<Section>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_md: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    backlinks: Option<Vec<BacklinkContext>>,
    attributions: Vec<SectionAttribution>,
    degraded: bool,
}

#[derive(Debug, Clone, Serialize)]
struct SectionAttribution {
    section_id: String,
    author_id: String,
    author_type: EditorType,
    timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Deserialize)]
struct DocSectionsParams {
    workspace_id: Uuid,
    doc_id: Uuid,
}

#[derive(Debug, Clone, Serialize)]
struct DocSectionsResult {
    doc_id: Uuid,
    sections: Vec<Section>,
}

#[derive(Debug, Clone, Deserialize)]
struct DocTreeParams {
    workspace_id: Uuid,
    #[serde(default)]
    path_prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct DocTreeEntry {
    doc_id: Uuid,
    path: String,
    title: String,
}

#[derive(Debug, Clone, Serialize)]
struct DocTreeResult {
    items: Vec<DocTreeEntry>,
    total: usize,
}

const DOC_SEARCH_DEFAULT_LIMIT: usize = 20;
const DOC_SEARCH_MAX_LIMIT: usize = 100;

fn default_doc_search_limit() -> usize {
    DOC_SEARCH_DEFAULT_LIMIT
}

#[derive(Debug, Clone, Deserialize)]
struct DocSearchParams {
    workspace_id: Uuid,
    q: String,
    #[serde(default = "default_doc_search_limit")]
    limit: usize,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct DocSearchHit {
    doc_id: Uuid,
    path: String,
    title: String,
    snippet: String,
    score: f64,
}

#[derive(Debug, Clone, Serialize)]
struct DocSearchResult {
    items: Vec<DocSearchHit>,
    total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct DocDiffParams {
    workspace_id: Uuid,
    doc_id: Uuid,
    #[serde(default)]
    from_seq: Option<i64>,
    #[serde(default)]
    to_seq: Option<i64>,
    #[serde(default)]
    granularity: DocDiffGranularity,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
enum DocDiffGranularity {
    #[default]
    Snapshot,
    Coarse,
    Fine,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DocDiffSnapshotAttribution {
    author_id: String,
    author_type: EditorType,
    timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DocDiffAuthorshipSegment {
    author_id: String,
    author_type: EditorType,
    start_offset: usize,
    end_offset: usize,
    timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DocDiffSnapshot {
    seq: i64,
    timestamp: chrono::DateTime<chrono::Utc>,
    content_md: String,
    #[serde(default)]
    author_attributions: Vec<DocDiffSnapshotAttribution>,
    #[serde(default)]
    authorship_segments: Vec<DocDiffAuthorshipSegment>,
}

#[derive(Debug, Clone, Serialize)]
struct DocDiffResult {
    patch_md: String,
    from_seq: i64,
    to_seq: i64,
    granularity: DocDiffGranularity,
    snapshots: Vec<DocDiffSnapshot>,
}

#[derive(Debug, Clone, Deserialize)]
struct DocHistoryParams {
    workspace_id: Uuid,
    doc_id: Uuid,
    #[serde(default)]
    from_seq: Option<i64>,
    #[serde(default)]
    to_seq: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct DocHistoryEvent {
    seq: i64,
    author_id: String,
    author_type: EditorType,
    timestamp: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct DocHistoryResult {
    events: Vec<DocHistoryEvent>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
enum DocBundleInclude {
    Parents,
    Children,
    Backlinks,
    Comments,
}

#[derive(Debug, Clone, Deserialize)]
struct DocBundleParams {
    workspace_id: Uuid,
    doc_id: Uuid,
    #[serde(default)]
    section_id: Option<String>,
    #[serde(default)]
    include: Vec<DocBundleInclude>,
    #[serde(default)]
    token_budget: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BacklinkContext {
    doc_id: Uuid,
    path: String,
    snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CommentThreadContext {
    id: String,
    workspace_id: String,
    doc_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    section_id: Option<String>,
    status: String,
    version: i64,
    created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DocBundleContext {
    #[serde(default)]
    parents: Vec<Section>,
    #[serde(default)]
    children: Vec<Section>,
    #[serde(default)]
    backlinks: Vec<BacklinkContext>,
    #[serde(default)]
    comments: Vec<CommentThreadContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DocBundleResult {
    section_content: String,
    context: DocBundleContext,
    tokens_used: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct DocEditSectionParams {
    workspace_id: Uuid,
    doc_id: Uuid,
    section: String,
    content: String,
    agent: String,
    #[serde(default)]
    summary: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct DocEditSectionResult {
    doc_path: String,
    section_id: String,
    heading: String,
    bytes_written: usize,
    etag: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DocEditParams {
    workspace_id: Uuid,
    doc_id: Uuid,
    client_update_id: String,
    #[serde(default)]
    ops: Option<serde_json::Value>,
    #[serde(default)]
    content_md: Option<String>,
    #[serde(default)]
    if_etag: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct DocEditResult {
    etag: String,
    head_seq: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct AgentStatusParams {
    workspace_id: Uuid,
}

#[derive(Debug, Clone, Deserialize)]
struct AgentConflictsParams {
    workspace_id: Uuid,
    #[serde(default)]
    doc_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
struct AgentListParams {
    workspace_id: Uuid,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AgentClaimMode {
    Exclusive,
    Shared,
}

#[derive(Debug, Clone, Deserialize)]
struct AgentClaimParams {
    workspace_id: Uuid,
    doc_id: Uuid,
    section_id: String,
    ttl_sec: u32,
    mode: AgentClaimMode,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct AgentWhoamiResult {
    agent_id: String,
    capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct AgentStatusResult {
    active_sessions: Vec<RpcAgentSession>,
    change_token: String,
}

#[derive(Debug, Clone, Serialize)]
struct AgentListItem {
    agent_id: String,
    last_seen_at: chrono::DateTime<chrono::Utc>,
    active_sections: u32,
}

#[derive(Debug, Clone, Serialize)]
struct AgentListResult {
    items: Vec<AgentListItem>,
}

#[derive(Debug, Clone, Serialize)]
struct AgentClaimConflictResult {
    agent_id: String,
    section_id: String,
}

#[derive(Debug, Clone, Serialize)]
struct AgentClaimResult {
    lease_id: String,
    expires_at: chrono::DateTime<chrono::Utc>,
    conflicts: Vec<AgentClaimConflictResult>,
}

// ── Workspace types ────────────────────────────────────────────────

/// In-memory workspace registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub workspace_id: Uuid,
    pub name: String,
    pub root_path: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkspaceListParams {
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_workspace_list_limit")]
    limit: usize,
}

fn default_workspace_list_limit() -> usize {
    20
}

fn workspace_to_rpc_workspace(workspace: &WorkspaceInfo) -> RpcWorkspace {
    let mut slug = slugify(&workspace.name);
    if slug.is_empty() {
        slug = workspace.workspace_id.simple().to_string();
    }

    RpcWorkspace {
        id: workspace.workspace_id,
        slug,
        name: workspace.name.clone(),
        role: None,
        created_at: workspace.created_at,
        updated_at: workspace.created_at,
        etag: format!(
            "workspace:{}:{}",
            workspace.workspace_id,
            workspace.created_at.timestamp_millis()
        ),
    }
}

#[derive(Debug, Clone, Serialize)]
struct WorkspaceListItem {
    #[serde(flatten)]
    workspace: RpcWorkspace,
    workspace_id: Uuid,
    root_path: String,
    doc_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct WorkspaceListResult {
    items: Vec<WorkspaceListItem>,
    next_cursor: Option<String>,
    total: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkspaceOpenParams {
    #[serde(default)]
    workspace_id: Option<Uuid>,
    #[serde(default)]
    root_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct WorkspaceOpenResult {
    workspace: RpcWorkspace,
    root_path: String,
    workspace_id: Uuid,
    name: String,
    doc_count: usize,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkspaceCreateParams {
    name: String,
    root_path: String,
}

#[derive(Debug, Clone, Serialize)]
struct WorkspaceCreateResult {
    workspace: RpcWorkspace,
    workspace_id: Uuid,
    name: String,
    root_path: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceStartupRecoveryReport {
    pub registered_workspaces: usize,
    pub skipped_paths: usize,
}

#[derive(Debug, Clone)]
struct ImportedWorkspaceDoc {
    doc_id: Uuid,
    path: String,
    abs_path: PathBuf,
    content: String,
    line_ending_style: String,
    last_fs_mtime_ns: i64,
    content_hash: String,
    title: String,
    tags: Vec<String>,
}

const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

fn is_markdown_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

fn collect_markdown_files_recursive(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];

    while let Some(dir) = pending.pop() {
        let entries = fs::read_dir(&dir)
            .map_err(|error| format!("failed to scan directory `{}`: {error}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                format!("failed to read directory entry in `{}`: {error}", dir.display())
            })?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|error| {
                format!(
                    "failed to inspect path `{}` while importing workspace: {error}",
                    path.display()
                )
            })?;

            if file_type.is_dir() {
                if entry.file_name() == ".scriptum" {
                    continue;
                }
                pending.push(path);
                continue;
            }

            if file_type.is_file() && is_markdown_file(&path) {
                files.push(path);
            }
        }
    }

    files.sort();
    Ok(files)
}

fn normalize_markdown_utf8(raw: &[u8]) -> String {
    let without_bom = raw.strip_prefix(&UTF8_BOM).unwrap_or(raw);
    String::from_utf8_lossy(without_bom).into_owned()
}

fn detect_line_ending_style(content: &str) -> String {
    if content.contains("\r\n") {
        "crlf".to_string()
    } else {
        "lf".to_string()
    }
}

fn modified_to_unix_nanos(path: &Path) -> Result<i64, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("failed to read metadata for `{}`: {error}", path.display()))?;
    let modified = metadata.modified().map_err(|error| {
        format!("failed to read modification time for `{}`: {error}", path.display())
    })?;
    let nanos = modified
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("invalid modification time for `{}`: {error}", path.display()))?
        .as_nanos();
    i64::try_from(nanos).map_err(|_| format!("modification time overflow for `{}`", path.display()))
}

fn ensure_unique_path_norm(raw_paths: &[String]) -> Result<(), String> {
    let mut seen = HashMap::<String, String>::new();
    for raw in raw_paths {
        let path_norm = normalize_path(raw)
            .map_err(|error| format!("invalid markdown path `{raw}`: {error}"))?;
        if let Some(first) = seen.insert(path_norm.clone(), raw.clone()) {
            return Err(format!(
                "path collision after normalization for `{path_norm}`: `{first}` and `{raw}`"
            ));
        }
    }
    Ok(())
}

fn tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?m)(?:^|[\s(\[])#([A-Za-z0-9][A-Za-z0-9_-]{0,63})\b")
            .expect("tag extraction regex should compile")
    })
}

fn extract_index_tags(content: &str) -> Vec<String> {
    let mut tags = BTreeSet::new();
    for captures in tag_regex().captures_iter(content) {
        if let Some(tag) = captures.get(1) {
            tags.insert(tag.as_str().to_ascii_lowercase());
        }
    }
    tags.into_iter().collect()
}

fn ensure_tag_schema(conn: &rusqlite::Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tags (
            name TEXT PRIMARY KEY
        );
        CREATE TABLE IF NOT EXISTS document_tags (
            doc_id TEXT NOT NULL,
            tag_name TEXT NOT NULL,
            PRIMARY KEY (doc_id, tag_name)
        );
        CREATE INDEX IF NOT EXISTS document_tags_tag_idx
            ON document_tags (tag_name);",
    )
    .map_err(|error| format!("failed to ensure tag index schema: {error}"))
}

fn replace_document_tags(
    conn: &rusqlite::Connection,
    doc_id: &str,
    tags: &[String],
) -> Result<(), String> {
    conn.execute("DELETE FROM document_tags WHERE doc_id = ?1", rusqlite::params![doc_id])
        .map_err(|error| format!("failed to clear existing tags for doc `{doc_id}`: {error}"))?;

    for tag in tags {
        conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", rusqlite::params![tag])
            .map_err(|error| format!("failed to upsert tag `{tag}`: {error}"))?;
        conn.execute(
            "INSERT OR IGNORE INTO document_tags (doc_id, tag_name) VALUES (?1, ?2)",
            rusqlite::params![doc_id, tag],
        )
        .map_err(|error| format!("failed to link tag `{tag}` to doc `{doc_id}`: {error}"))?;
    }

    Ok(())
}

fn scan_workspace_markdown_docs(root: &Path) -> Result<Vec<ImportedWorkspaceDoc>, String> {
    let files = collect_markdown_files_recursive(root)?;
    let relative_paths: Vec<String> = files
        .iter()
        .map(|path| {
            path.strip_prefix(root)
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                .map_err(|_| {
                    format!(
                        "imported file `{}` is outside workspace root `{}`",
                        path.display(),
                        root.display()
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    ensure_unique_path_norm(&relative_paths)?;

    let mut imported = Vec::with_capacity(files.len());
    for (abs_path, relative_path) in files.into_iter().zip(relative_paths.into_iter()) {
        let path_norm = normalize_path(&relative_path)
            .map_err(|error| format!("invalid markdown path `{relative_path}`: {error}"))?;
        let raw = fs::read(&abs_path)
            .map_err(|error| format!("failed to read `{}`: {error}", abs_path.display()))?;
        let content = normalize_markdown_utf8(&raw);
        let content_hash = sha256_hex(content.as_bytes());
        let title = extract_title(&content, Path::new(&path_norm));
        let tags = extract_index_tags(&content);
        imported.push(ImportedWorkspaceDoc {
            doc_id: Uuid::new_v4(),
            path: path_norm,
            abs_path: abs_path.clone(),
            line_ending_style: detect_line_ending_style(&content),
            last_fs_mtime_ns: modified_to_unix_nanos(&abs_path)?,
            content_hash,
            title,
            tags,
            content,
        });
    }

    Ok(imported)
}

impl Default for RpcServerState {
    fn default() -> Self {
        let meta_db = MetaDb::open(":memory:").expect("in-memory meta.db should initialize");
        let lease_store = LeaseStore::new(meta_db.connection(), chrono::Utc::now())
            .expect("lease store should initialize");
        let default_crdt_store_dir = std::env::temp_dir().join("scriptum").join("crdt_store");
        Self {
            doc_manager: Arc::new(RwLock::new(DocManager::default())),
            doc_metadata: Arc::new(RwLock::new(HashMap::new())),
            doc_history: Arc::new(RwLock::new(HashMap::new())),
            degraded_docs: Arc::new(RwLock::new(HashSet::new())),
            crdt_store_dir: Arc::new(default_crdt_store_dir),
            global_config_path: None,
            workspaces: Arc::new(RwLock::new(HashMap::new())),
            shutdown_notifier: None,
            git_state: None,
            git_triggers: Arc::new(Mutex::new(TriggerCollector::new(TriggerConfig::default()))),
            git_idle_timer_epoch: Arc::new(AtomicU64::new(0)),
            agent_db: Arc::new(Mutex::new(meta_db)),
            lease_store: Arc::new(Mutex::new(lease_store)),
            agent_id: Arc::new("local-agent".to_string()),
        }
    }
}

impl RpcServerState {
    /// Expose doc_manager for integration tests (e.g., CRDT sync verification).
    pub fn doc_manager_for_test(&self) -> &Arc<RwLock<DocManager>> {
        &self.doc_manager
    }

    pub async fn recover_docs_at_startup(
        &self,
        crdt_store_dir: impl AsRef<std::path::Path>,
    ) -> Result<StartupRecoveryReport, String> {
        let report = {
            let mut manager = self.doc_manager.write().await;
            recover_documents_into_manager(crdt_store_dir.as_ref(), &mut manager)
                .map_err(|error| error.to_string())?
        };

        {
            let mut degraded_docs = self.degraded_docs.write().await;
            degraded_docs.clear();
            degraded_docs.extend(report.degraded_docs.iter().copied());
        }

        Ok(report)
    }

    pub async fn is_doc_degraded_for_test(&self, doc_id: Uuid) -> bool {
        self.degraded_docs.read().await.contains(&doc_id)
    }

    pub fn with_shutdown_notifier(mut self, shutdown_notifier: broadcast::Sender<()>) -> Self {
        self.shutdown_notifier = Some(shutdown_notifier);
        self
    }

    pub fn with_git_state<E: CommandExecutor + 'static>(mut self, git: GitState<E>) -> Self {
        self.git_state = Some(Arc::new(git));
        self
    }

    #[cfg(test)]
    fn with_git_trigger_config(mut self, config: TriggerConfig) -> Self {
        self.git_triggers = Arc::new(Mutex::new(TriggerCollector::new(config)));
        self
    }

    pub fn with_agent_identity(mut self, agent_id: impl Into<String>) -> Self {
        self.agent_id = Arc::new(agent_id.into());
        self
    }

    pub fn with_crdt_store_dir(mut self, crdt_store_dir: impl Into<PathBuf>) -> Self {
        self.crdt_store_dir = Arc::new(crdt_store_dir.into());
        self
    }

    pub fn with_global_config_path(mut self, global_config_path: impl Into<PathBuf>) -> Self {
        self.global_config_path = Some(global_config_path.into());
        self
    }

    fn with_agent_storage<T, F>(&self, f: F) -> Result<T, String>
    where
        F: FnOnce(&rusqlite::Connection, &mut LeaseStore) -> Result<T, String>,
    {
        let db = self.agent_db.lock().map_err(|_| "agent db lock poisoned".to_string())?;
        let mut leases =
            self.lease_store.lock().map_err(|_| "agent lease store lock poisoned".to_string())?;
        f(db.connection(), &mut leases)
    }

    fn register_git_change(&self, path: &str) {
        if self.git_state.is_none() {
            return;
        }
        let normalized = path.trim();
        if normalized.is_empty() {
            return;
        }

        if let Ok(mut collector) = self.git_triggers.lock() {
            collector.mark_changed(normalized);
        }
        self.schedule_idle_fallback_commit();
    }

    fn enqueue_git_trigger(&self, trigger: TriggerEvent) {
        if self.git_state.is_none() {
            return;
        }
        if let Ok(mut collector) = self.git_triggers.lock() {
            collector.push_trigger(trigger);
        }
    }

    fn clear_trigger_state_after_commit(&self) {
        let Ok(mut collector) = self.git_triggers.lock() else {
            return;
        };
        let tracked = collector.tracked_changed_files();
        let _ = collector.take_commit_context_at(Instant::now(), tracked);
    }

    async fn run_triggered_auto_commit(&self) -> Result<Option<Uuid>, String> {
        let Some(git) = &self.git_state else {
            return Ok(None);
        };

        let policy = git.get_policy();
        if matches!(policy, GitSyncPolicy::Disabled) {
            return Ok(None);
        }

        let (message, trigger_type) = {
            let mut collector = self
                .git_triggers
                .lock()
                .map_err(|_| "git trigger collector lock poisoned".to_string())?;
            let now = Instant::now();
            if !collector.should_commit(now) {
                return Ok(None);
            }

            let changed_files = collector.tracked_changed_files();
            if changed_files.is_empty() {
                return Ok(None);
            }

            let Some(context) = collector.take_commit_context_at(now, changed_files) else {
                return Ok(None);
            };
            (context.generate_message(), context.trigger.kind().to_string())
        };

        let action = match policy {
            GitSyncPolicy::Disabled => return Ok(None),
            GitSyncPolicy::Manual => {
                GitSyncAction::Commit { message, trigger_type: Some(trigger_type) }
            }
            GitSyncPolicy::AutoRebase => {
                GitSyncAction::CommitAndPush { message, trigger_type: Some(trigger_type) }
            }
        };

        git.sync(action).await.map(Some)
    }

    fn schedule_idle_fallback_commit(&self) {
        if self.git_state.is_none() {
            return;
        }

        let idle_timeout = self
            .git_triggers
            .lock()
            .ok()
            .map(|collector| collector.idle_fallback_timeout())
            .unwrap_or_else(|| Duration::from_secs(30));

        let epoch = self.git_idle_timer_epoch.fetch_add(1, Ordering::SeqCst) + 1;
        let state = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(idle_timeout).await;
            if state.git_idle_timer_epoch.load(Ordering::SeqCst) != epoch {
                return;
            }
            if let Err(error) = state.run_triggered_auto_commit().await {
                warn!(error = %error, "idle fallback trigger auto-commit failed");
            }
        });
    }

    fn schedule_lease_expiry_trigger(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        section_id: String,
        agent_id: String,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) {
        if self.git_state.is_none() {
            return;
        }

        let doc_path = self
            .doc_metadata
            .try_read()
            .ok()
            .and_then(|metadata| {
                metadata.get(&(workspace_id, doc_id)).map(|record| record.path.clone())
            })
            .unwrap_or_else(|| format!("{doc_id}.md"));

        let workspace_key = workspace_id.to_string();
        let doc_key = doc_id.to_string();
        let delay = expires_at
            .signed_duration_since(chrono::Utc::now())
            .to_std()
            .unwrap_or_else(|_| Duration::from_secs(0));

        let state = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;

            let agent_id_for_check = agent_id.clone();
            let should_trigger = state.with_agent_storage(|conn, lease_store| {
                let active = lease_store
                    .active_leases_for_section(
                        conn,
                        workspace_key.as_str(),
                        doc_key.as_str(),
                        section_id.as_str(),
                        chrono::Utc::now(),
                    )
                    .map_err(|error| error.to_string())?;
                Ok(!active.iter().any(|lease| lease.agent_id == agent_id_for_check))
            });

            match should_trigger {
                Ok(true) => {
                    state.enqueue_git_trigger(TriggerEvent::LeaseReleased {
                        agent: agent_id.clone(),
                        doc_path: doc_path.clone(),
                        section_heading: section_id.clone(),
                    });
                    if let Err(error) = state.run_triggered_auto_commit().await {
                        warn!(error = %error, "lease expiry trigger auto-commit failed");
                    }
                }
                Ok(false) => {}
                Err(error) => {
                    warn!(error = %error, "failed to evaluate lease expiry trigger");
                }
            }
        });
    }

    fn maybe_enqueue_comment_resolved_trigger(
        &self,
        client_update_id: &str,
        doc_path: &str,
        section_id: &str,
        agent_id: Option<&str>,
    ) {
        if self.git_state.is_none() {
            return;
        }

        let thread_id = client_update_id
            .strip_prefix("comment.resolve:")
            .or_else(|| client_update_id.strip_prefix("comment_resolved:"))
            .or_else(|| client_update_id.strip_prefix("comment:resolve:"));

        let Some(thread_id) = thread_id.map(str::trim).filter(|id| !id.is_empty()) else {
            return;
        };

        let agent = agent_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| self.agent_id.as_ref().as_str())
            .to_string();
        let section_hint = section_id.trim();
        let doc_path = doc_path.to_string();
        let thread_id = thread_id.to_string();

        self.enqueue_git_trigger(TriggerEvent::CommentResolved {
            agent,
            doc_path: doc_path.clone(),
            thread_id,
        });

        if !section_hint.is_empty() {
            self.register_git_change(doc_path.as_str());
        }

        let state = self.clone();
        tokio::spawn(async move {
            if let Err(error) = state.run_triggered_auto_commit().await {
                warn!(error = %error, "comment resolution trigger auto-commit failed");
            }
        });
    }

    fn ensure_search_index<'a>(conn: &'a rusqlite::Connection) -> Result<Fts5Index<'a>, String> {
        let search_index = Fts5Index::new(conn);
        search_index
            .ensure_schema()
            .map_err(|error| format!("failed to ensure FTS index schema: {error}"))?;
        Ok(search_index)
    }

    fn upsert_search_index_entry(
        conn: &rusqlite::Connection,
        doc_id: Uuid,
        title: &str,
        content: &str,
    ) -> Result<(), String> {
        let search_index = Self::ensure_search_index(conn)?;
        search_index
            .upsert(&IndexEntry {
                doc_id: doc_id.to_string(),
                title: title.to_string(),
                content: content.to_string(),
            })
            .map_err(|error| format!("failed to update search index for doc {doc_id}: {error}"))
    }

    fn search_index_hits(
        conn: &rusqlite::Connection,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchHit>, String> {
        let search_index = Self::ensure_search_index(conn)?;
        search_index.search(query, limit).map_err(|error| format!("failed to search docs: {error}"))
    }

    fn load_bundle_backlinks(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        metadata_by_doc_id: &HashMap<String, DocMetadataRecord>,
    ) -> Result<Vec<BacklinkContext>, String> {
        self.with_agent_storage(|conn, _| {
            let backlink_store = BacklinkStore::new(conn);
            backlink_store
                .ensure_schema()
                .map_err(|error| format!("failed to ensure backlink index schema: {error}"))?;

            let incoming =
                backlink_store.incoming_for_target(&doc_id.to_string()).map_err(|error| {
                    format!("failed to query incoming backlinks for doc {doc_id}: {error}")
                })?;

            let mut backlinks = incoming
                .into_iter()
                .filter_map(|backlink| {
                    let metadata = metadata_by_doc_id.get(&backlink.source_doc_id)?;
                    if metadata.workspace_id != workspace_id {
                        return None;
                    }
                    let source_doc_id = Uuid::parse_str(&backlink.source_doc_id).ok()?;
                    Some(BacklinkContext {
                        doc_id: source_doc_id,
                        path: metadata.path.clone(),
                        snippet: backlink.link_text,
                    })
                })
                .collect::<Vec<_>>();
            backlinks.sort_by(|left, right| left.path.cmp(&right.path));
            Ok(backlinks)
        })
    }

    fn load_bundle_comments(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        target_section: Option<&Section>,
        sections_by_id: &HashMap<String, Section>,
    ) -> Result<Vec<CommentThreadContext>, String> {
        self.with_agent_storage(|conn, _| {
            if !sqlite_table_exists(conn, "comment_threads")
                .map_err(|error| format!("failed to inspect comment_threads schema: {error}"))?
            {
                return Ok(Vec::new());
            }

            let mut stmt = conn
                .prepare(
                    "SELECT id, workspace_id, doc_id, section_id, status, version, created_at, resolved_at
                     FROM comment_threads
                     WHERE workspace_id = ?1 AND doc_id = ?2
                     ORDER BY created_at DESC",
                )
                .map_err(|error| format!("failed to prepare comment thread bundle query: {error}"))?;
            let rows = stmt
                .query_map(rusqlite::params![workspace_id.to_string(), doc_id.to_string()], |row| {
                    Ok(CommentThreadContext {
                        id: row.get(0)?,
                        workspace_id: row.get(1)?,
                        doc_id: row.get(2)?,
                        section_id: row.get(3)?,
                        status: row.get(4)?,
                        version: row.get(5)?,
                        created_at: row.get(6)?,
                        resolved_at: row.get(7)?,
                    })
                })
                .map_err(|error| format!("failed to query comment threads for bundle: {error}"))?;

            let mut comments = rows
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|error| format!("failed to decode comment threads for bundle: {error}"))?;
            comments.retain(|comment| {
                comment_matches_target_section(
                    comment.section_id.as_deref(),
                    target_section,
                    sections_by_id,
                )
            });
            Ok(comments)
        })
    }

    fn ensure_active_session(
        conn: &rusqlite::Connection,
        workspace_id: Uuid,
        agent_id: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), String> {
        let workspace = workspace_id.to_string();
        let active_sessions =
            SessionStore::list_active(conn, &workspace).map_err(|error| error.to_string())?;

        if let Some(session) =
            active_sessions.into_iter().find(|session| session.agent_id == agent_id)
        {
            SessionStore::touch(conn, &session.session_id, now)
                .map_err(|error| error.to_string())?;
            return Ok(());
        }

        let session = PersistedAgentSession {
            session_id: format!("rpc-{}", Uuid::new_v4()),
            agent_id: agent_id.to_string(),
            workspace_id: workspace,
            started_at: now,
            last_seen_at: now,
            status: SessionStatus::Active,
        };
        SessionStore::create(conn, &session).map_err(|error| error.to_string())
    }

    fn count_active_sections_by_agent(
        leases: &[crate::agent::lease::SectionLease],
    ) -> HashMap<String, u32> {
        let mut dedupe: HashMap<String, HashSet<(String, String)>> = HashMap::new();
        for lease in leases {
            dedupe
                .entry(lease.agent_id.clone())
                .or_default()
                .insert((lease.doc_id.clone(), lease.section_id.clone()));
        }

        dedupe.into_iter().map(|(agent_id, sections)| (agent_id, sections.len() as u32)).collect()
    }

    fn build_workspace_change_token(
        workspace_id: Uuid,
        active_sessions: &[RpcAgentSession],
        workspace_doc_heads: &[(Uuid, i64)],
    ) -> String {
        let mut fingerprint = String::new();
        fingerprint.push_str("workspace:");
        fingerprint.push_str(&workspace_id.to_string());
        fingerprint.push('\n');

        let mut session_fingerprint = active_sessions
            .iter()
            .map(|session| {
                format!(
                    "{}|{}|{}",
                    session.agent_id,
                    session.last_seen_at.to_rfc3339(),
                    session.active_sections
                )
            })
            .collect::<Vec<_>>();
        session_fingerprint.sort();
        for line in session_fingerprint {
            fingerprint.push_str("session:");
            fingerprint.push_str(&line);
            fingerprint.push('\n');
        }

        for (doc_id, head_seq) in workspace_doc_heads {
            fingerprint.push_str("doc:");
            fingerprint.push_str(&doc_id.to_string());
            fingerprint.push(':');
            fingerprint.push_str(&head_seq.to_string());
            fingerprint.push('\n');
        }

        sha256_hex(fingerprint.as_bytes())
    }

    async fn agent_status(&self, workspace_id: Uuid) -> Result<AgentStatusResult, String> {
        let now = chrono::Utc::now();
        let active_sessions = self.with_agent_storage(|conn, lease_store| {
            let workspace = workspace_id.to_string();
            let sessions =
                SessionStore::list_active(conn, &workspace).map_err(|error| error.to_string())?;
            let leases = lease_store
                .active_leases(conn, &workspace, None, now)
                .map_err(|error| error.to_string())?;
            let active_sections_by_agent = Self::count_active_sections_by_agent(&leases);

            let active_sessions = sessions
                .into_iter()
                .map(|session| RpcAgentSession {
                    agent_id: session.agent_id.clone(),
                    workspace_id,
                    last_seen_at: session.last_seen_at,
                    active_sections: active_sections_by_agent
                        .get(&session.agent_id)
                        .copied()
                        .unwrap_or(0),
                })
                .collect::<Vec<_>>();
            Ok(active_sessions)
        })?;

        let workspace_doc_heads = {
            let metadata = self.doc_metadata.read().await;
            let mut doc_heads = metadata
                .iter()
                .filter(|((ws_id, _), _)| *ws_id == workspace_id)
                .map(|((_, doc_id), record)| (*doc_id, record.head_seq))
                .collect::<Vec<_>>();
            doc_heads.sort_by_key(|(doc_id, _)| *doc_id);
            doc_heads
        };
        let change_token = Self::build_workspace_change_token(
            workspace_id,
            &active_sessions,
            &workspace_doc_heads,
        );

        Ok(AgentStatusResult { active_sessions, change_token })
    }

    fn agent_list(&self, workspace_id: Uuid) -> Result<AgentListResult, String> {
        let now = chrono::Utc::now();
        self.with_agent_storage(|conn, lease_store| {
            let workspace = workspace_id.to_string();
            let sessions =
                SessionStore::list_active(conn, &workspace).map_err(|error| error.to_string())?;
            let leases = lease_store
                .active_leases(conn, &workspace, None, now)
                .map_err(|error| error.to_string())?;
            let active_sections_by_agent = Self::count_active_sections_by_agent(&leases);

            let mut items_by_agent: HashMap<String, chrono::DateTime<chrono::Utc>> = HashMap::new();
            for session in sessions {
                items_by_agent
                    .entry(session.agent_id)
                    .and_modify(|last_seen| {
                        if session.last_seen_at > *last_seen {
                            *last_seen = session.last_seen_at;
                        }
                    })
                    .or_insert(session.last_seen_at);
            }

            let mut items = items_by_agent
                .into_iter()
                .map(|(agent_id, last_seen_at)| AgentListItem {
                    active_sections: active_sections_by_agent.get(&agent_id).copied().unwrap_or(0),
                    agent_id,
                    last_seen_at,
                })
                .collect::<Vec<_>>();
            items.sort_by(|a, b| {
                b.last_seen_at.cmp(&a.last_seen_at).then_with(|| a.agent_id.cmp(&b.agent_id))
            });

            Ok(AgentListResult { items })
        })
    }

    fn agent_conflicts(
        &self,
        workspace_id: Uuid,
        doc_id: Option<Uuid>,
    ) -> Result<Vec<SectionOverlap>, String> {
        let now = chrono::Utc::now();
        self.with_agent_storage(|conn, lease_store| {
            let workspace = workspace_id.to_string();
            let doc_filter = doc_id.map(|value| value.to_string());
            let leases = lease_store
                .active_leases(conn, &workspace, doc_filter.as_deref(), now)
                .map_err(|error| error.to_string())?;

            let mut grouped: HashMap<(String, String), Vec<crate::agent::lease::SectionLease>> =
                HashMap::new();
            for lease in leases {
                grouped
                    .entry((lease.doc_id.clone(), lease.section_id.clone()))
                    .or_default()
                    .push(lease);
            }

            let mut items = grouped
                .into_iter()
                .filter_map(|((_doc_id, section_id), mut section_leases)| {
                    if section_leases.len() < 2 {
                        return None;
                    }

                    section_leases.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
                    let editors = section_leases
                        .iter()
                        .map(|lease| OverlapEditor {
                            name: lease.agent_id.clone(),
                            editor_type: EditorType::Agent,
                            cursor_offset: 0,
                            last_edit_at: lease.expires_at,
                        })
                        .collect::<Vec<_>>();

                    let section = Section {
                        id: section_id.clone(),
                        parent_id: None,
                        heading: section_id.clone(),
                        level: 1,
                        start_line: 1,
                        end_line: 2,
                    };

                    Some(SectionOverlap { section, editors, severity: OverlapSeverity::Info })
                })
                .collect::<Vec<_>>();
            items.sort_by(|a, b| a.section.id.cmp(&b.section.id));
            Ok(items)
        })
    }

    fn agent_claim(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        section_id: String,
        ttl_sec: u32,
        mode: AgentClaimMode,
        note: Option<String>,
        agent_id: Option<String>,
    ) -> Result<AgentClaimResult, String> {
        let normalized_section_id = section_id.trim().to_string();
        if normalized_section_id.is_empty() {
            return Err("section_id must not be empty".to_string());
        }

        let now = chrono::Utc::now();
        let agent_id = agent_id.unwrap_or_else(|| (*self.agent_id).clone());
        let mode = match mode {
            AgentClaimMode::Exclusive => LeaseMode::Exclusive,
            AgentClaimMode::Shared => LeaseMode::Shared,
        };

        let (result, claimed_section_id, claimed_agent_id, claimed_expires_at) = self
            .with_agent_storage(|conn, lease_store| {
                Self::ensure_active_session(conn, workspace_id, &agent_id, now)?;

                let claim = LeaseClaim {
                    workspace_id: workspace_id.to_string(),
                    doc_id: doc_id.to_string(),
                    section_id: normalized_section_id.clone(),
                    agent_id: agent_id.clone(),
                    ttl_sec,
                    mode,
                    note,
                };

                let claim_result =
                    lease_store.claim(conn, claim, now).map_err(|error| error.to_string())?;
                let lease = claim_result.lease;
                let conflicts = claim_result
                    .conflicts
                    .into_iter()
                    .map(|conflict| AgentClaimConflictResult {
                        agent_id: conflict.agent_id,
                        section_id: conflict.section_id,
                    })
                    .collect::<Vec<_>>();

                Ok((
                    AgentClaimResult {
                        lease_id: format!(
                            "{}:{}:{}:{}",
                            workspace_id, doc_id, lease.section_id, lease.agent_id
                        ),
                        expires_at: lease.expires_at,
                        conflicts,
                    },
                    lease.section_id,
                    lease.agent_id,
                    lease.expires_at,
                ))
            })?;

        self.schedule_lease_expiry_trigger(
            workspace_id,
            doc_id,
            claimed_section_id,
            claimed_agent_id,
            claimed_expires_at,
        );

        Ok(result)
    }

    pub async fn seed_doc(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        path: impl Into<String>,
        title: impl Into<String>,
        markdown: impl AsRef<str>,
    ) {
        let markdown = markdown.as_ref();
        let doc = YDoc::new();
        if !markdown.is_empty() {
            doc.insert_text("content", 0, markdown);
        }

        {
            let mut manager = self.doc_manager.write().await;
            manager.put_doc(doc_id, doc);
        }

        let path = path.into();
        let title = title.into();
        let metadata = DocMetadataRecord {
            workspace_id,
            doc_id,
            path,
            title: title.clone(),
            head_seq: 0,
            etag: format!("doc:{doc_id}:0"),
        };
        self.doc_metadata.write().await.insert((workspace_id, doc_id), metadata);
        self.record_doc_snapshot(workspace_id, doc_id, 0, markdown).await;
        self.with_agent_storage(|conn, _| {
            Self::upsert_search_index_entry(conn, doc_id, &title, markdown)?;
            Ok(())
        })
        .expect("seeded docs should be indexed in FTS");
        self.degraded_docs.write().await.remove(&doc_id);
    }

    /// Register a workspace (for tests).
    pub async fn seed_workspace(
        &self,
        workspace_id: Uuid,
        name: impl Into<String>,
        root_path: impl Into<String>,
    ) {
        let info = WorkspaceInfo {
            workspace_id,
            name: name.into(),
            root_path: root_path.into(),
            created_at: chrono::Utc::now(),
        };
        self.workspaces.write().await.insert(workspace_id, info);
    }

    async fn record_doc_snapshot(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        seq: i64,
        content_md: &str,
    ) {
        self.record_doc_snapshot_with_metadata(
            workspace_id,
            doc_id,
            seq,
            content_md,
            HISTORY_SYSTEM_AUTHOR_ID,
            EditorType::Agent,
            None,
        )
        .await;
    }

    async fn record_doc_snapshot_with_metadata(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        seq: i64,
        content_md: &str,
        author_id: &str,
        author_type: EditorType,
        summary: Option<&str>,
    ) {
        let mut history = self.doc_history.write().await;
        history.entry((workspace_id, doc_id)).or_default().entry(seq).or_insert_with(|| {
            DocSnapshotRecord {
                content_md: content_md.to_string(),
                timestamp: chrono::Utc::now(),
                author_id: author_id.to_string(),
                author_type,
                summary: summary.map(str::to_string),
            }
        });
    }

    fn append_doc_wal_update(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        payload: &[u8],
    ) -> Result<(), String> {
        let wal_root = self.crdt_store_dir.join("wal");
        let wal = WalStore::for_doc(&wal_root, workspace_id, doc_id)
            .map_err(|error| format!("failed to open WAL for doc {doc_id}: {error}"))?;
        wal.append_update(payload)
            .map_err(|error| format!("failed to append WAL update for doc {doc_id}: {error}"))
    }

    async fn workspace_list(&self, offset: usize, limit: usize) -> WorkspaceListResult {
        let workspaces = self.workspaces.read().await;
        let doc_metadata = self.doc_metadata.read().await;

        // Count docs per workspace.
        let mut doc_counts: HashMap<Uuid, usize> = HashMap::new();
        for (ws_id, _doc_id) in doc_metadata.keys() {
            *doc_counts.entry(*ws_id).or_default() += 1;
        }

        let mut items: Vec<WorkspaceListItem> = workspaces
            .values()
            .map(|ws| WorkspaceListItem {
                workspace: workspace_to_rpc_workspace(ws),
                workspace_id: ws.workspace_id,
                root_path: ws.root_path.clone(),
                doc_count: doc_counts.get(&ws.workspace_id).copied().unwrap_or(0),
            })
            .collect();
        items.sort_by(|a, b| a.workspace.name.cmp(&b.workspace.name));

        let total = items.len();
        let items: Vec<WorkspaceListItem> = items.into_iter().skip(offset).take(limit).collect();
        let next_offset = offset.saturating_add(items.len());
        let next_cursor = (next_offset < total).then(|| next_offset.to_string());

        WorkspaceListResult { items, next_cursor, total }
    }

    async fn workspace_doc_count(&self, workspace_id: Uuid) -> usize {
        let doc_metadata = self.doc_metadata.read().await;
        doc_metadata.keys().filter(|(ws_id, _)| *ws_id == workspace_id).count()
    }

    async fn workspace_open_by_id(
        &self,
        workspace_id: Uuid,
    ) -> Result<WorkspaceOpenResult, String> {
        let ws = {
            let workspaces = self.workspaces.read().await;
            workspaces
                .get(&workspace_id)
                .cloned()
                .ok_or_else(|| format!("workspace {} not found", workspace_id))?
        };
        let doc_count = self.workspace_doc_count(workspace_id).await;
        let workspace = workspace_to_rpc_workspace(&ws);

        Ok(WorkspaceOpenResult {
            workspace,
            root_path: ws.root_path.clone(),
            workspace_id: ws.workspace_id,
            name: ws.name.clone(),
            doc_count,
            created_at: ws.created_at,
        })
    }

    fn load_registered_workspace_paths(&self) -> Vec<String> {
        let Some(path) = self.global_config_path.as_deref() else {
            return Vec::new();
        };

        match GlobalConfig::load_from(path) {
            Ok(config) => config.workspace_paths,
            Err(crate::config::ConfigError::Io(error))
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                Vec::new()
            }
            Err(error) => {
                warn!(
                    path = %path.display(),
                    error = %error,
                    "failed to load global config while restoring workspace registry; using defaults"
                );
                Vec::new()
            }
        }
    }

    fn persist_registered_workspace_path(&self, root_path: &str) -> Result<(), String> {
        let Some(path) = self.global_config_path.as_deref() else {
            return Ok(());
        };

        let mut config = match GlobalConfig::load_from(path) {
            Ok(config) => config,
            Err(crate::config::ConfigError::Io(error))
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                GlobalConfig::default()
            }
            Err(error) => {
                warn!(
                    path = %path.display(),
                    error = %error,
                    "failed to load global config while registering workspace path; using defaults"
                );
                GlobalConfig::default()
            }
        };

        if !config.add_workspace_path(root_path) {
            return Ok(());
        }

        config.save_to(path).map_err(|error| {
            format!("failed to persist workspace path in `{}`: {error}", path.display())
        })
    }

    fn canonical_workspace_root(raw_path: &str) -> Result<PathBuf, String> {
        let trimmed = raw_path.trim();
        if trimmed.is_empty() {
            return Err("root_path must not be empty".to_string());
        }

        let path = Path::new(trimmed);
        if !path.is_absolute() {
            return Err("root_path must be an absolute path".to_string());
        }

        let canonical = path.canonicalize().map_err(|error| {
            format!("failed to resolve workspace root `{}`: {error}", path.display())
        })?;
        if !canonical.is_dir() {
            return Err(format!("workspace root `{}` is not a directory", canonical.display()));
        }
        Ok(canonical)
    }

    fn workspace_info_from_root(root_path: &Path) -> Result<WorkspaceInfo, String> {
        let workspace_toml = workspace_config_path(root_path);
        if !workspace_toml.is_file() {
            return Err(format!("workspace config not found at `{}`", workspace_toml.display()));
        }

        let config = WorkspaceConfig::load_from(&workspace_toml).map_err(|error| {
            format!("failed to read workspace config `{}`: {error}", workspace_toml.display())
        })?;

        let raw_workspace_id = config.sync.workspace_id.as_deref().ok_or_else(|| {
            format!("workspace config `{}` is missing sync.workspace_id", workspace_toml.display())
        })?;
        let workspace_id = Uuid::parse_str(raw_workspace_id).map_err(|error| {
            format!(
                "workspace config `{}` has invalid sync.workspace_id `{raw_workspace_id}`: {error}",
                workspace_toml.display()
            )
        })?;

        let name = config
            .sync
            .workspace_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                root_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "Workspace".to_string());

        let canonical_root = root_path.to_str().ok_or_else(|| {
            format!("workspace root path `{}` is not valid UTF-8", root_path.display())
        })?;

        Ok(WorkspaceInfo {
            workspace_id,
            name,
            root_path: canonical_root.to_string(),
            created_at: chrono::Utc::now(),
        })
    }

    async fn register_workspace_from_root_path(
        &self,
        raw_root_path: &str,
        persist_registration: bool,
    ) -> Result<WorkspaceInfo, String> {
        let canonical_root = Self::canonical_workspace_root(raw_root_path)?;
        let info = Self::workspace_info_from_root(&canonical_root)?;

        {
            let mut workspaces = self.workspaces.write().await;
            if let Some(duplicate_id) = workspaces
                .values()
                .find(|workspace| {
                    workspace.root_path == info.root_path
                        && workspace.workspace_id != info.workspace_id
                })
                .map(|workspace| workspace.workspace_id)
            {
                workspaces.remove(&duplicate_id);
            }
            workspaces.insert(info.workspace_id, info.clone());
        }

        if persist_registration {
            self.persist_registered_workspace_path(&info.root_path)?;
        }

        Ok(info)
    }

    pub async fn recover_workspaces_at_startup(&self) -> WorkspaceStartupRecoveryReport {
        let mut report = WorkspaceStartupRecoveryReport::default();
        let registered_paths = self.load_registered_workspace_paths();
        let mut unique_paths = BTreeSet::new();

        for root_path in registered_paths {
            if !unique_paths.insert(root_path.clone()) {
                continue;
            }

            match self.register_workspace_from_root_path(&root_path, false).await {
                Ok(_) => report.registered_workspaces += 1,
                Err(error) => {
                    report.skipped_paths += 1;
                    warn!(
                        root_path = %root_path,
                        error = %error,
                        "skipping workspace registration during startup"
                    );
                }
            }
        }

        report
    }

    async fn workspace_open(
        &self,
        params: WorkspaceOpenParams,
    ) -> Result<WorkspaceOpenResult, String> {
        match (params.workspace_id, params.root_path) {
            (Some(workspace_id), None) => self.workspace_open_by_id(workspace_id).await,
            (None, Some(root_path)) => {
                let info = self.register_workspace_from_root_path(&root_path, true).await?;
                self.workspace_open_by_id(info.workspace_id).await
            }
            (Some(_), Some(_)) => {
                Err("workspace.open accepts either workspace_id or root_path, not both".to_string())
            }
            (None, None) => Err("workspace.open requires workspace_id or root_path".to_string()),
        }
    }

    async fn workspace_create(
        &self,
        name: String,
        root_path: String,
    ) -> Result<WorkspaceCreateResult, String> {
        let trimmed_name = name.trim().to_string();
        if trimmed_name.is_empty() {
            return Err("workspace name must not be empty".to_string());
        }

        let path = std::path::Path::new(&root_path);
        if !path.is_absolute() {
            return Err("root_path must be an absolute path".to_string());
        }

        let workspace_id = Uuid::new_v4();
        let created_at = chrono::Utc::now();

        // Create `.scriptum/` directory and required workspace-local stores.
        let scriptum_dir = path.join(".scriptum");
        let workspace_crdt_store_dir = scriptum_dir.join("crdt_store");
        let wal_dir = workspace_crdt_store_dir.join("wal");
        let snapshots_dir = workspace_crdt_store_dir.join("snapshots");

        std::fs::create_dir_all(&wal_dir)
            .map_err(|e| format!("failed to create .scriptum wal directory: {e}"))?;
        std::fs::create_dir_all(&snapshots_dir)
            .map_err(|e| format!("failed to create .scriptum snapshots directory: {e}"))?;

        let workspace_meta_db_path = scriptum_dir.join("meta.db");
        MetaDb::open(&workspace_meta_db_path)
            .map_err(|e| format!("failed to initialize workspace meta.db: {e}"))?;

        let mut config = crate::config::WorkspaceConfig::default();
        config.sync.workspace_id = Some(workspace_id.to_string());
        config.sync.workspace_name = Some(trimmed_name.clone());
        config.save(path).map_err(|e| format!("failed to write workspace.toml: {e}"))?;
        let canonical_root_path = path.canonicalize().map_err(|error| {
            format!("failed to resolve workspace root `{}`: {error}", path.display())
        })?;
        let canonical_root = canonical_root_path
            .to_str()
            .ok_or_else(|| {
                format!(
                    "workspace root path `{}` is not valid UTF-8",
                    canonical_root_path.display()
                )
            })?
            .to_string();

        // Import any existing markdown files from disk.
        let imported_docs = scan_workspace_markdown_docs(&canonical_root_path)?;

        // Seed local metadata/index stores for imported docs.
        self.with_agent_storage(|conn, _| {
            let search_index = Self::ensure_search_index(conn)?;

            let backlink_store = BacklinkStore::new(conn);
            backlink_store
                .ensure_schema()
                .map_err(|error| format!("failed to ensure backlink index schema: {error}"))?;

            ensure_tag_schema(conn)?;

            let linkables: Vec<LinkableDocument> = imported_docs
                .iter()
                .map(|doc| LinkableDocument {
                    doc_id: doc.doc_id.to_string(),
                    path: doc.path.clone(),
                    title: Some(doc.title.clone()),
                })
                .collect();

            for doc in &imported_docs {
                let local_record = LocalDocumentRecord {
                    doc_id: doc.doc_id.to_string(),
                    workspace_id: workspace_id.to_string(),
                    abs_path: doc.abs_path.to_string_lossy().to_string(),
                    line_ending_style: doc.line_ending_style.clone(),
                    last_fs_mtime_ns: doc.last_fs_mtime_ns,
                    last_content_hash: doc.content_hash.clone(),
                    projection_rev: 0,
                    last_server_seq: 0,
                    last_ack_seq: 0,
                    parse_error: None,
                };
                DocumentsLocalStore::insert(conn, &local_record).map_err(|error| {
                    format!("failed to register imported local doc `{}`: {error}", doc.path)
                })?;

                search_index
                    .upsert(&IndexEntry {
                        doc_id: doc.doc_id.to_string(),
                        title: doc.title.clone(),
                        content: doc.content.clone(),
                    })
                    .map_err(|error| {
                        format!("failed to index imported doc `{}`: {error}", doc.path)
                    })?;

                let links = parse_wiki_links(&doc.content);
                let resolved = resolve_wiki_links(&doc.doc_id.to_string(), &links, &linkables);
                backlink_store.replace_for_source(&doc.doc_id.to_string(), &resolved).map_err(
                    |error| format!("failed to index backlinks for `{}`: {error}", doc.path),
                )?;

                replace_document_tags(conn, &doc.doc_id.to_string(), &doc.tags)?;
            }

            Ok(())
        })?;

        // Seed in-memory CRDT docs + RPC metadata/history with seq=0.
        {
            let mut manager = self.doc_manager.write().await;
            for doc in &imported_docs {
                let ydoc = manager.subscribe_or_create(doc.doc_id);
                ydoc.insert_text("content", 0, &doc.content);
                let _ = manager.unsubscribe(doc.doc_id);
            }
        }

        {
            let mut metadata = self.doc_metadata.write().await;
            for doc in &imported_docs {
                metadata.insert(
                    (workspace_id, doc.doc_id),
                    DocMetadataRecord {
                        workspace_id,
                        doc_id: doc.doc_id,
                        path: doc.path.clone(),
                        title: doc.title.clone(),
                        head_seq: 0,
                        etag: format!("doc:{}:0", doc.doc_id),
                    },
                );
            }
        }

        {
            let mut history = self.doc_history.write().await;
            for doc in &imported_docs {
                history.entry((workspace_id, doc.doc_id)).or_default().insert(
                    0,
                    DocSnapshotRecord {
                        content_md: doc.content.clone(),
                        timestamp: chrono::Utc::now(),
                        author_id: HISTORY_SYSTEM_AUTHOR_ID.to_string(),
                        author_type: EditorType::Agent,
                        summary: Some("workspace import".to_string()),
                    },
                );
            }
        }

        let info = WorkspaceInfo {
            workspace_id,
            name: trimmed_name.clone(),
            root_path: canonical_root.clone(),
            created_at,
        };
        self.workspaces.write().await.insert(workspace_id, info.clone());
        self.persist_registered_workspace_path(&canonical_root)?;
        let workspace = workspace_to_rpc_workspace(&info);

        Ok(WorkspaceCreateResult {
            workspace,
            workspace_id,
            name: trimmed_name,
            root_path: canonical_root,
            created_at,
        })
    }

    async fn read_doc(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        include_content: bool,
        include_backlinks: bool,
    ) -> DocReadResult {
        let doc = {
            let mut manager = self.doc_manager.write().await;
            manager.subscribe_or_create(doc_id)
        };

        let metadata = {
            let mut metadata = self.doc_metadata.write().await;
            metadata
                .entry((workspace_id, doc_id))
                .or_insert_with(|| default_metadata(workspace_id, doc_id))
                .clone()
        };
        let document = metadata_to_rpc_document(&metadata);

        let content_md = doc.get_text_string("content");
        let sections = parse_sections(&content_md);
        self.record_doc_snapshot(workspace_id, doc_id, metadata.head_seq, &content_md).await;
        let degraded = self.degraded_docs.read().await.contains(&doc_id);
        let backlinks = if include_backlinks {
            let metadata_by_doc_id = {
                let metadata = self.doc_metadata.read().await;
                metadata
                    .values()
                    .filter(|record| record.workspace_id == workspace_id)
                    .cloned()
                    .map(|record| (record.doc_id.to_string(), record))
                    .collect::<HashMap<_, _>>()
            };
            match self.load_bundle_backlinks(workspace_id, doc_id, &metadata_by_doc_id) {
                Ok(backlinks) => Some(backlinks),
                Err(error) => {
                    warn!(
                        doc_id = %doc_id,
                        workspace_id = %workspace_id,
                        error = %error,
                        "failed to load backlinks for doc.read"
                    );
                    Some(Vec::new())
                }
            }
        } else {
            None
        };

        {
            let mut manager = self.doc_manager.write().await;
            let _ = manager.unsubscribe(doc_id);
        }

        DocReadResult {
            document,
            sections,
            content_md: include_content.then_some(content_md),
            backlinks,
            attributions: Vec::new(),
            degraded,
        }
    }

    async fn edit_doc(&self, params: DocEditParams) -> Result<DocEditResult, String> {
        let doc = {
            let mut manager = self.doc_manager.write().await;
            manager.subscribe_or_create(params.doc_id)
        };

        let outcome = async {
            if params.client_update_id.trim().is_empty() {
                return Err("client_update_id must not be empty".to_string());
            }
            if let Some(agent_id) = params.agent_id.as_deref() {
                if agent_id.trim().is_empty() {
                    return Err("agent_id must not be empty".to_string());
                }
            }
            if params.content_md.is_none() && params.ops.is_none() {
                return Err("doc.edit requires either `ops` or `content_md`".to_string());
            }

            let current_head_seq = {
                let mut metadata = self.doc_metadata.write().await;
                let record = metadata
                    .entry((params.workspace_id, params.doc_id))
                    .or_insert_with(|| default_metadata(params.workspace_id, params.doc_id));
                if let Some(if_etag) = params.if_etag.as_deref() {
                    if if_etag != record.etag {
                        return Err(format!(
                            "if_etag mismatch: expected `{}`, got `{}`",
                            record.etag, if_etag
                        ));
                    }
                }
                record.head_seq
            };
            self.record_doc_snapshot(
                params.workspace_id,
                params.doc_id,
                current_head_seq,
                &doc.get_text_string("content"),
            )
            .await;

            let current_state = doc.encode_state();
            let staged_doc = YDoc::from_state(&current_state)
                .map_err(|error| format!("failed to stage doc state for WAL append: {error}"))?;

            if let Some(content_md) = params.content_md.as_deref() {
                let existing_len = staged_doc.text_len("content");
                staged_doc.replace_text("content", 0, existing_len, content_md);
            }

            if let Some(ops_value) = params.ops.as_ref() {
                let update_bytes = decode_doc_edit_ops(ops_value)?;
                staged_doc
                    .apply_update(&update_bytes)
                    .map_err(|error| format!("failed to apply Yjs ops: {error}"))?;
            }

            let wal_update = staged_doc.encode_state();
            self.append_doc_wal_update(params.workspace_id, params.doc_id, &wal_update)?;
            doc.apply_update(&wal_update)
                .map_err(|error| format!("failed to apply staged Yjs update: {error}"))?;

            let updated_content = doc.get_text_string("content");
            let (result, updated_seq, updated_title, updated_path) = {
                let mut metadata = self.doc_metadata.write().await;
                let record = metadata
                    .entry((params.workspace_id, params.doc_id))
                    .or_insert_with(|| default_metadata(params.workspace_id, params.doc_id));
                record.head_seq = record.head_seq.saturating_add(1);
                record.etag = format!("doc:{}:{}", params.doc_id, record.head_seq);
                let normalized_title =
                    extract_title(&updated_content, Path::new(record.path.as_str()));
                record.title = normalized_title.clone();
                (
                    DocEditResult { etag: record.etag.clone(), head_seq: record.head_seq },
                    record.head_seq,
                    normalized_title,
                    record.path.clone(),
                )
            };
            let snapshot_author_id = params
                .agent_id
                .clone()
                .unwrap_or_else(|| HISTORY_LOCAL_HUMAN_AUTHOR_ID.to_string());
            let snapshot_author_type =
                if params.agent_id.is_some() { EditorType::Agent } else { EditorType::Human };
            let snapshot_summary = params.client_update_id.clone();
            self.record_doc_snapshot_with_metadata(
                params.workspace_id,
                params.doc_id,
                updated_seq,
                &updated_content,
                &snapshot_author_id,
                snapshot_author_type,
                Some(snapshot_summary.as_str()),
            )
            .await;
            let source_doc_id = params.doc_id.to_string();
            let linkable_docs = {
                let metadata = self.doc_metadata.read().await;
                metadata
                    .values()
                    .filter(|record| record.workspace_id == params.workspace_id)
                    .map(|record| LinkableDocument {
                        doc_id: record.doc_id.to_string(),
                        path: record.path.clone(),
                        title: Some(record.title.clone()),
                    })
                    .collect::<Vec<_>>()
            };
            let parsed_links = parse_wiki_links(&updated_content);
            let resolved_backlinks =
                resolve_wiki_links(&source_doc_id, &parsed_links, &linkable_docs);
            if let Err(error) = self.with_agent_storage(|conn, _| {
                Self::upsert_search_index_entry(
                    conn,
                    params.doc_id,
                    &updated_title,
                    &updated_content,
                )?;
                let backlink_store = BacklinkStore::new(conn);
                backlink_store
                    .ensure_schema()
                    .map_err(|error| format!("failed to ensure backlink index schema: {error}"))?;
                backlink_store.replace_for_source(&source_doc_id, &resolved_backlinks).map_err(
                    |error| {
                        format!("failed to update backlinks for doc {}: {error}", params.doc_id)
                    },
                )?;
                Ok(())
            }) {
                warn!(
                    doc_id = %params.doc_id,
                    workspace_id = %params.workspace_id,
                    error = %error,
                    "failed to update persistent search/backlink indexes after doc.edit"
                );
            }

            self.register_git_change(updated_path.as_str());
            self.maybe_enqueue_comment_resolved_trigger(
                params.client_update_id.as_str(),
                updated_path.as_str(),
                "",
                params.agent_id.as_deref(),
            );

            Ok(result)
        }
        .await;

        {
            let mut manager = self.doc_manager.write().await;
            let _ = manager.unsubscribe(params.doc_id);
        }

        outcome
    }

    async fn doc_sections(&self, workspace_id: Uuid, doc_id: Uuid) -> DocSectionsResult {
        let doc = {
            let mut manager = self.doc_manager.write().await;
            manager.subscribe_or_create(doc_id)
        };

        let content = doc.get_text_string("content");
        let sections = parse_sections(&content);

        {
            let mut manager = self.doc_manager.write().await;
            let _ = manager.unsubscribe(doc_id);
        }

        // Ensure metadata entry exists (mirrors read_doc behavior).
        let head_seq = {
            let mut metadata = self.doc_metadata.write().await;
            metadata
                .entry((workspace_id, doc_id))
                .or_insert_with(|| default_metadata(workspace_id, doc_id))
                .head_seq
        };
        self.record_doc_snapshot(workspace_id, doc_id, head_seq, &content).await;

        DocSectionsResult { doc_id, sections }
    }

    async fn doc_tree(&self, workspace_id: Uuid, path_prefix: Option<&str>) -> DocTreeResult {
        let metadata = self.doc_metadata.read().await;

        let mut items: Vec<DocTreeEntry> = metadata
            .values()
            .filter(|m| m.workspace_id == workspace_id)
            .filter(|m| match path_prefix {
                Some(prefix) => m.path.starts_with(prefix),
                None => true,
            })
            .map(|m| DocTreeEntry {
                doc_id: m.doc_id,
                path: m.path.clone(),
                title: m.title.clone(),
            })
            .collect();

        items.sort_by(|a, b| a.path.cmp(&b.path));
        let total = items.len();

        DocTreeResult { items, total }
    }

    async fn doc_search(&self, params: DocSearchParams) -> Result<DocSearchResult, String> {
        let query = params.q.trim();
        if query.is_empty() {
            return Err("q must not be empty".to_string());
        }
        if params.limit == 0 || params.limit > DOC_SEARCH_MAX_LIMIT {
            return Err(format!("limit must be between 1 and {}", DOC_SEARCH_MAX_LIMIT));
        }

        let offset = decode_doc_search_cursor(params.cursor.as_deref())?;
        let (metadata_by_doc_id, search_limit) = {
            let metadata = self.doc_metadata.read().await;
            let search_limit = metadata.len().min(i64::MAX as usize).max(1);
            let metadata_by_doc_id = metadata
                .values()
                .filter(|record| record.workspace_id == params.workspace_id)
                .cloned()
                .map(|record| (record.doc_id.to_string(), record))
                .collect::<HashMap<_, _>>();
            (metadata_by_doc_id, search_limit)
        };

        if metadata_by_doc_id.is_empty() {
            return Ok(DocSearchResult { items: Vec::new(), total: 0, next_cursor: None });
        }

        let workspace_hits: Vec<SearchHit> = self.with_agent_storage(|conn, _| {
            let hits = Self::search_index_hits(conn, query, search_limit)?;
            Ok(hits
                .into_iter()
                .filter(|hit| metadata_by_doc_id.contains_key(&hit.doc_id))
                .collect())
        })?;
        if offset > workspace_hits.len() {
            return Err(format!("cursor offset {offset} is out of range"));
        }

        let end = offset.saturating_add(params.limit).min(workspace_hits.len());
        let mut items = Vec::with_capacity(end.saturating_sub(offset));
        for hit in &workspace_hits[offset..end] {
            if let Some(record) = metadata_by_doc_id.get(&hit.doc_id) {
                items.push(DocSearchHit {
                    doc_id: record.doc_id,
                    path: record.path.clone(),
                    title: record.title.clone(),
                    snippet: hit.snippet.clone(),
                    score: hit.rank,
                });
            }
        }

        let next_cursor = (end < workspace_hits.len()).then(|| encode_doc_search_cursor(end));
        Ok(DocSearchResult { items, total: workspace_hits.len(), next_cursor })
    }

    async fn doc_diff(&self, params: DocDiffParams) -> Result<DocDiffResult, String> {
        let head_seq = {
            let mut metadata = self.doc_metadata.write().await;
            metadata
                .entry((params.workspace_id, params.doc_id))
                .or_insert_with(|| default_metadata(params.workspace_id, params.doc_id))
                .head_seq
        };
        let (from_seq, to_seq) =
            resolve_doc_history_range(head_seq, params.from_seq, params.to_seq)?;

        let mut snapshots = {
            let history = self.doc_history.read().await;
            let doc_history = history.get(&(params.workspace_id, params.doc_id));
            (
                doc_history.and_then(|h| h.get(&from_seq).cloned()),
                doc_history.and_then(|h| h.get(&to_seq).cloned()),
            )
        };

        if snapshots.0.is_none() || snapshots.1.is_none() {
            let doc = {
                let mut manager = self.doc_manager.write().await;
                manager.subscribe_or_create(params.doc_id)
            };
            let current_content = doc.get_text_string("content");
            self.record_doc_snapshot(
                params.workspace_id,
                params.doc_id,
                head_seq,
                &current_content,
            )
            .await;
            {
                let mut manager = self.doc_manager.write().await;
                let _ = manager.unsubscribe(params.doc_id);
            }

            let history = self.doc_history.read().await;
            if let Some(doc_history) = history.get(&(params.workspace_id, params.doc_id)) {
                snapshots.0 = snapshots.0.or_else(|| doc_history.get(&from_seq).cloned());
                snapshots.1 = snapshots.1.or_else(|| doc_history.get(&to_seq).cloned());
            }
        }

        let Some(from_snapshot) = snapshots.0 else {
            return Err(format!(
                "sequence {from_seq} is unavailable for doc {} (head_seq={head_seq})",
                params.doc_id
            ));
        };
        let Some(to_snapshot) = snapshots.1 else {
            return Err(format!(
                "sequence {to_seq} is unavailable for doc {} (head_seq={head_seq})",
                params.doc_id
            ));
        };

        let timeline_snapshots = {
            let history = self.doc_history.read().await;
            history
                .get(&(params.workspace_id, params.doc_id))
                .map(|doc_history| {
                    doc_history
                        .range(from_seq..=to_seq)
                        .map(|(seq, snapshot)| snapshot_to_diff_snapshot(*seq, snapshot))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };

        Ok(DocDiffResult {
            patch_md: render_markdown_patch(&from_snapshot.content_md, &to_snapshot.content_md),
            from_seq,
            to_seq,
            granularity: params.granularity,
            snapshots: timeline_snapshots,
        })
    }

    async fn doc_history(&self, params: DocHistoryParams) -> Result<DocHistoryResult, String> {
        let head_seq = {
            let mut metadata = self.doc_metadata.write().await;
            metadata
                .entry((params.workspace_id, params.doc_id))
                .or_insert_with(|| default_metadata(params.workspace_id, params.doc_id))
                .head_seq
        };
        let (from_seq, to_seq) =
            resolve_doc_history_range(head_seq, params.from_seq, params.to_seq)?;

        let has_range_snapshots = {
            let history = self.doc_history.read().await;
            history
                .get(&(params.workspace_id, params.doc_id))
                .map(|doc_history| {
                    doc_history.contains_key(&from_seq) && doc_history.contains_key(&to_seq)
                })
                .unwrap_or(false)
        };

        if !has_range_snapshots {
            let doc = {
                let mut manager = self.doc_manager.write().await;
                manager.subscribe_or_create(params.doc_id)
            };
            let current_content = doc.get_text_string("content");
            self.record_doc_snapshot(
                params.workspace_id,
                params.doc_id,
                head_seq,
                &current_content,
            )
            .await;
            {
                let mut manager = self.doc_manager.write().await;
                let _ = manager.unsubscribe(params.doc_id);
            }
        }

        let events = {
            let history = self.doc_history.read().await;
            history
                .get(&(params.workspace_id, params.doc_id))
                .map(|doc_history| {
                    doc_history
                        .range(from_seq..=to_seq)
                        .map(|(seq, snapshot)| DocHistoryEvent {
                            seq: *seq,
                            author_id: snapshot.author_id.clone(),
                            author_type: snapshot.author_type,
                            timestamp: snapshot.timestamp,
                            summary: snapshot.summary.clone(),
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };

        Ok(DocHistoryResult { events })
    }

    async fn bundle_doc(&self, params: DocBundleParams) -> Result<DocBundleResult, String> {
        let doc = {
            let mut manager = self.doc_manager.write().await;
            manager.subscribe_or_create(params.doc_id)
        };
        let doc_id = params.doc_id;
        let metadata_by_doc_id = {
            let metadata = self.doc_metadata.read().await;
            metadata
                .values()
                .filter(|record| record.workspace_id == params.workspace_id)
                .cloned()
                .map(|record| (record.doc_id.to_string(), record))
                .collect::<HashMap<_, _>>()
        };

        let bundle_result = (|| -> Result<DocBundleResult, String> {
            let content_md = doc.get_text_string("content");
            let sections = parse_sections(&content_md);
            let sections_by_id: HashMap<String, Section> =
                sections.iter().cloned().map(|section| (section.id.clone(), section)).collect();

            let target_section = if let Some(section_id) = params.section_id.as_deref() {
                let normalized = section_id.trim();
                if normalized.is_empty() {
                    return Err("section_id must not be empty".to_string());
                }

                Some(
                    sections_by_id
                        .get(normalized)
                        .cloned()
                        .ok_or_else(|| format!("section `{normalized}` not found"))?,
                )
            } else {
                None
            };

            let include: HashSet<DocBundleInclude> = params.include.into_iter().collect();
            let mut context = DocBundleContext::default();

            if include.contains(&DocBundleInclude::Parents) {
                if let Some(target) = target_section.as_ref() {
                    context.parents = section_parent_chain(target, &sections_by_id);
                }
            }

            if include.contains(&DocBundleInclude::Children) {
                if let Some(target) = target_section.as_ref() {
                    context.children = section_descendants(target, &sections, &sections_by_id);
                }
            }

            if include.contains(&DocBundleInclude::Backlinks) {
                context.backlinks = self.load_bundle_backlinks(
                    params.workspace_id,
                    params.doc_id,
                    &metadata_by_doc_id,
                )?;
            }

            if include.contains(&DocBundleInclude::Comments) {
                context.comments = self.load_bundle_comments(
                    params.workspace_id,
                    params.doc_id,
                    target_section.as_ref(),
                    &sections_by_id,
                )?;
            }

            let section_content = extract_section_content(&content_md, target_section.as_ref());
            let tokens_used =
                apply_bundle_token_budget(&section_content, &mut context, params.token_budget)?;

            Ok(DocBundleResult { section_content, context, tokens_used })
        })();

        {
            let mut manager = self.doc_manager.write().await;
            let _ = manager.unsubscribe(doc_id);
        }

        bundle_result
    }

    async fn edit_section(
        &self,
        params: DocEditSectionParams,
    ) -> Result<DocEditSectionResult, String> {
        let doc = {
            let mut manager = self.doc_manager.write().await;
            manager.subscribe_or_create(params.doc_id)
        };

        let content = doc.get_text_string("content");
        let sections = parse_sections(&content);

        // Find the target section by heading (strip leading # from the param for matching).
        let section_heading = params.section.trim_start_matches('#').trim();
        let section = sections
            .iter()
            .find(|s| s.heading == section_heading)
            .ok_or_else(|| format!("section `{}` not found", params.section))?;

        // Calculate byte offsets for the section body.
        // The heading line is at `start_line`. The body starts right after the heading line.
        // The section ends at `end_line` (exclusive — it's the start_line of the next section,
        // or total_lines + 1 for the last section).
        let lines: Vec<&str> = content.lines().collect();
        let heading_line_idx = (section.start_line - 1) as usize;
        let end_line_idx = (section.end_line - 1) as usize;

        // Body starts after the heading line.
        let body_start_line = heading_line_idx + 1;
        let body_end_line = end_line_idx.min(lines.len());

        // Calculate character offsets.
        let mut char_offset = 0u32;
        let mut body_start_offset = 0u32;
        let mut body_end_offset;

        for (i, line) in content.split('\n').enumerate() {
            if i == body_start_line {
                body_start_offset = char_offset;
            }
            if i == body_end_line {
                break;
            }
            char_offset += line.len() as u32 + 1; // +1 for the newline
        }
        body_end_offset = char_offset;

        // Handle the case where body_start_line >= lines.len() (section at end of doc with no body).
        if body_start_line >= content.split('\n').count() {
            body_start_offset = content.len() as u32;
            body_end_offset = content.len() as u32;
        }

        // Replace the body text in the CRDT.
        let body_len = body_end_offset.saturating_sub(body_start_offset);
        doc.replace_text("content", body_start_offset, body_len, &params.content);

        let new_content = doc.get_text_string("content");
        let section_id = section.id.clone();
        let heading = section.heading.clone();

        // Update metadata etag.
        let doc_path = {
            let metadata = self.doc_metadata.read().await;
            metadata
                .get(&(params.workspace_id, params.doc_id))
                .map(|m| m.path.clone())
                .unwrap_or_else(|| format!("{}.md", params.doc_id))
        };

        let new_etag = format!("doc:{}:{}", params.doc_id, new_content.len());
        {
            let mut metadata = self.doc_metadata.write().await;
            if let Some(record) = metadata.get_mut(&(params.workspace_id, params.doc_id)) {
                record.etag = new_etag.clone();
            }
        }

        {
            let mut manager = self.doc_manager.write().await;
            let _ = manager.unsubscribe(params.doc_id);
        }

        Ok(DocEditSectionResult {
            doc_path,
            section_id,
            heading,
            bytes_written: params.content.len(),
            etag: new_etag,
        })
    }
}

pub async fn handle_raw_request(raw: &[u8], state: &RpcServerState) -> Response {
    let trace_id = trace_id_from_raw_request(raw);
    with_trace_id_scope(trace_id.clone(), async {
        let request = match serde_json::from_slice::<Request>(raw) {
            Ok(request) => request,
            Err(error) => {
                return Response::error(
                    RequestId::Null,
                    RpcError {
                        code: PARSE_ERROR,
                        message: "Parse error".to_string(),
                        data: Some(json!({ "reason": error.to_string() })),
                    },
                );
            }
        };

        if request.jsonrpc != "2.0" {
            return Response::error(
                request.id,
                RpcError {
                    code: INVALID_REQUEST,
                    message: "Invalid Request".to_string(),
                    data: None,
                },
            );
        }

        let protocol_version =
            request.protocol_version.as_deref().unwrap_or(RPC_CURRENT_PROTOCOL_VERSION);
        if !is_supported_protocol_version(protocol_version) {
            return Response::error(
                request.id,
                RpcError {
                    code: INVALID_REQUEST,
                    message: "Unsupported protocol version".to_string(),
                    data: Some(json!({
                        "requested_version": protocol_version,
                        "supported_versions": RPC_SUPPORTED_PROTOCOL_VERSIONS,
                        "current_version": RPC_CURRENT_PROTOCOL_VERSION,
                    })),
                },
            );
        }

        let request_method = request.method.clone();
        let request_id = request.id.clone();
        dispatch_request(request, state)
            .instrument(info_span!(
                "daemon.rpc.dispatch",
                rpc_method = %request_method,
                rpc_id = ?request_id
            ))
            .await
    })
    .instrument(info_span!("daemon.rpc.request", trace_id = %trace_id))
    .await
}

pub async fn dispatch_request(request: Request, state: &RpcServerState) -> Response {
    match request.method.as_str() {
        rpc_methods::RPC_PING => Response::success(
            request.id,
            json!({
                "ok": true,
            }),
        ),
        rpc_methods::DAEMON_SHUTDOWN => {
            if let Some(notifier) = &state.shutdown_notifier {
                let _ = notifier.send(());
            }
            Response::success(
                request.id,
                json!({
                    "ok": true,
                }),
            )
        }
        rpc_methods::DOC_READ => handle_doc_read(request, state).await,
        rpc_methods::DOC_EDIT => handle_doc_edit(request, state).await,
        rpc_methods::DOC_BUNDLE => handle_doc_bundle(request, state).await,
        rpc_methods::DOC_EDIT_SECTION => handle_doc_edit_section(request, state).await,
        rpc_methods::DOC_SECTIONS => handle_doc_sections(request, state).await,
        rpc_methods::DOC_DIFF => handle_doc_diff(request, state).await,
        rpc_methods::DOC_HISTORY => handle_doc_history(request, state).await,
        rpc_methods::DOC_SEARCH => handle_doc_search(request, state).await,
        rpc_methods::DOC_TREE => handle_doc_tree(request, state).await,
        rpc_methods::AGENT_WHOAMI => handle_agent_whoami(request, state),
        rpc_methods::AGENT_STATUS => handle_agent_status(request, state).await,
        rpc_methods::AGENT_CONFLICTS => handle_agent_conflicts(request, state),
        rpc_methods::AGENT_LIST => handle_agent_list(request, state),
        rpc_methods::AGENT_CLAIM => handle_agent_claim(request, state),
        rpc_methods::WORKSPACE_LIST => handle_workspace_list(request, state).await,
        rpc_methods::WORKSPACE_OPEN => handle_workspace_open(request, state).await,
        rpc_methods::WORKSPACE_CREATE => handle_workspace_create(request, state).await,
        rpc_methods::GIT_STATUS => handle_git_status(request, state),
        rpc_methods::GIT_SYNC => handle_git_sync(request, state).await,
        rpc_methods::GIT_CONFIGURE => handle_git_configure(request, state),
        "rpc.internal_error" => Response::error(
            request.id,
            RpcError { code: INTERNAL_ERROR, message: "Internal error".to_string(), data: None },
        ),
        _ => Response::error(
            request.id,
            RpcError {
                code: METHOD_NOT_FOUND,
                message: "Method not found".to_string(),
                data: None,
            },
        ),
    }
}

async fn handle_doc_read(request: Request, state: &RpcServerState) -> Response {
    let params = match parse_doc_read_params(request.params, request.id.clone()) {
        Ok(params) => params,
        Err(response) => return response,
    };

    let result = state
        .read_doc(
            params.workspace_id,
            params.doc_id,
            params.include_content,
            params.include_backlinks,
        )
        .await;
    Response::success(request.id, json!(result))
}

fn parse_doc_read_params(
    params: Option<serde_json::Value>,
    request_id: RequestId,
) -> Result<DocReadParams, Response> {
    let Some(params) = params else {
        return Err(invalid_params_response(request_id, "doc.read requires params".to_string()));
    };

    serde_json::from_value::<DocReadParams>(params).map_err(|error| {
        invalid_params_response(request_id, format!("failed to decode doc.read params: {}", error))
    })
}

async fn handle_doc_bundle(request: Request, state: &RpcServerState) -> Response {
    let params = match parse_doc_bundle_params(request.params, request.id.clone()) {
        Ok(params) => params,
        Err(response) => return response,
    };

    match state.bundle_doc(params).await {
        Ok(result) => Response::success(request.id, json!(result)),
        Err(reason) => invalid_params_response(request.id, reason),
    }
}

fn parse_doc_bundle_params(
    params: Option<serde_json::Value>,
    request_id: RequestId,
) -> Result<DocBundleParams, Response> {
    let Some(params) = params else {
        return Err(invalid_params_response(request_id, "doc.bundle requires params".to_string()));
    };

    serde_json::from_value::<DocBundleParams>(params).map_err(|error| {
        invalid_params_response(
            request_id,
            format!("failed to decode doc.bundle params: {}", error),
        )
    })
}

async fn handle_doc_edit(request: Request, state: &RpcServerState) -> Response {
    let params = match parse_doc_edit_params(request.params, request.id.clone()) {
        Ok(params) => params,
        Err(response) => return response,
    };

    match state.edit_doc(params).await {
        Ok(result) => Response::success(request.id, json!(result)),
        Err(reason) => invalid_params_response(request.id, reason),
    }
}

fn parse_doc_edit_params(
    params: Option<serde_json::Value>,
    request_id: RequestId,
) -> Result<DocEditParams, Response> {
    let Some(params) = params else {
        return Err(invalid_params_response(request_id, "doc.edit requires params".to_string()));
    };

    serde_json::from_value::<DocEditParams>(params).map_err(|error| {
        invalid_params_response(request_id, format!("failed to decode doc.edit params: {error}"))
    })
}

async fn handle_doc_edit_section(request: Request, state: &RpcServerState) -> Response {
    let Some(params) = request.params else {
        return invalid_params_response(request.id, "doc.edit_section requires params".to_string());
    };

    let params: DocEditSectionParams = match serde_json::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return invalid_params_response(
                request.id,
                format!("failed to decode doc.edit_section params: {e}"),
            );
        }
    };

    match state.edit_section(params).await {
        Ok(result) => Response::success(request.id, json!(result)),
        Err(e) => {
            Response::error(request.id, RpcError { code: INTERNAL_ERROR, message: e, data: None })
        }
    }
}

async fn handle_doc_sections(request: Request, state: &RpcServerState) -> Response {
    let Some(params) = request.params else {
        return invalid_params_response(request.id, "doc.sections requires params".to_string());
    };

    let params: DocSectionsParams = match serde_json::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return invalid_params_response(
                request.id,
                format!("failed to decode doc.sections params: {e}"),
            );
        }
    };

    let result = state.doc_sections(params.workspace_id, params.doc_id).await;
    Response::success(request.id, json!(result))
}

async fn handle_doc_diff(request: Request, state: &RpcServerState) -> Response {
    let params = match parse_doc_diff_params(request.params, request.id.clone()) {
        Ok(params) => params,
        Err(response) => return response,
    };

    match state.doc_diff(params).await {
        Ok(result) => Response::success(request.id, json!(result)),
        Err(reason) => invalid_params_response(request.id, reason),
    }
}

fn parse_doc_diff_params(
    params: Option<serde_json::Value>,
    request_id: RequestId,
) -> Result<DocDiffParams, Response> {
    let Some(params) = params else {
        return Err(invalid_params_response(request_id, "doc.diff requires params".to_string()));
    };

    serde_json::from_value::<DocDiffParams>(params).map_err(|error| {
        invalid_params_response(request_id, format!("failed to decode doc.diff params: {error}"))
    })
}

async fn handle_doc_history(request: Request, state: &RpcServerState) -> Response {
    let params = match parse_doc_history_params(request.params, request.id.clone()) {
        Ok(params) => params,
        Err(response) => return response,
    };

    match state.doc_history(params).await {
        Ok(result) => Response::success(request.id, json!(result)),
        Err(reason) => invalid_params_response(request.id, reason),
    }
}

fn parse_doc_history_params(
    params: Option<serde_json::Value>,
    request_id: RequestId,
) -> Result<DocHistoryParams, Response> {
    let Some(params) = params else {
        return Err(invalid_params_response(request_id, "doc.history requires params".to_string()));
    };

    serde_json::from_value::<DocHistoryParams>(params).map_err(|error| {
        invalid_params_response(request_id, format!("failed to decode doc.history params: {error}"))
    })
}

async fn handle_doc_search(request: Request, state: &RpcServerState) -> Response {
    let params = match parse_doc_search_params(request.params, request.id.clone()) {
        Ok(params) => params,
        Err(response) => return response,
    };

    match state.doc_search(params).await {
        Ok(result) => Response::success(request.id, json!(result)),
        Err(reason) => invalid_params_response(request.id, reason),
    }
}

fn parse_doc_search_params(
    params: Option<serde_json::Value>,
    request_id: RequestId,
) -> Result<DocSearchParams, Response> {
    let Some(params) = params else {
        return Err(invalid_params_response(request_id, "doc.search requires params".to_string()));
    };

    serde_json::from_value::<DocSearchParams>(params).map_err(|error| {
        invalid_params_response(request_id, format!("failed to decode doc.search params: {error}"))
    })
}

async fn handle_doc_tree(request: Request, state: &RpcServerState) -> Response {
    let Some(params) = request.params else {
        return invalid_params_response(request.id, "doc.tree requires params".to_string());
    };

    let params: DocTreeParams = match serde_json::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return invalid_params_response(
                request.id,
                format!("failed to decode doc.tree params: {e}"),
            );
        }
    };

    let result = state.doc_tree(params.workspace_id, params.path_prefix.as_deref()).await;
    Response::success(request.id, json!(result))
}

fn handle_agent_whoami(request: Request, state: &RpcServerState) -> Response {
    let result = AgentWhoamiResult {
        agent_id: (*state.agent_id).clone(),
        capabilities: vec![
            rpc_methods::AGENT_WHOAMI.to_string(),
            rpc_methods::AGENT_STATUS.to_string(),
            rpc_methods::AGENT_CONFLICTS.to_string(),
            rpc_methods::AGENT_LIST.to_string(),
            rpc_methods::AGENT_CLAIM.to_string(),
        ],
    };
    Response::success(request.id, json!(result))
}

async fn handle_agent_status(request: Request, state: &RpcServerState) -> Response {
    let params = match parse_agent_status_params(request.params, request.id.clone()) {
        Ok(params) => params,
        Err(response) => return response,
    };

    match state.agent_status(params.workspace_id).await {
        Ok(result) => Response::success(request.id, json!(result)),
        Err(reason) => Response::error(
            request.id,
            RpcError {
                code: INTERNAL_ERROR,
                message: format!("failed to read agent status: {reason}"),
                data: None,
            },
        ),
    }
}

fn parse_agent_status_params(
    params: Option<serde_json::Value>,
    request_id: RequestId,
) -> Result<AgentStatusParams, Response> {
    let Some(params) = params else {
        return Err(invalid_params_response(
            request_id,
            "agent.status requires params".to_string(),
        ));
    };

    serde_json::from_value::<AgentStatusParams>(params).map_err(|error| {
        invalid_params_response(
            request_id,
            format!("failed to decode agent.status params: {error}"),
        )
    })
}

fn handle_agent_conflicts(request: Request, state: &RpcServerState) -> Response {
    let params = match parse_agent_conflicts_params(request.params, request.id.clone()) {
        Ok(params) => params,
        Err(response) => return response,
    };

    match state.agent_conflicts(params.workspace_id, params.doc_id) {
        Ok(items) => Response::success(request.id, json!({ "items": items })),
        Err(reason) => Response::error(
            request.id,
            RpcError {
                code: INTERNAL_ERROR,
                message: format!("failed to read agent conflicts: {reason}"),
                data: None,
            },
        ),
    }
}

fn parse_agent_conflicts_params(
    params: Option<serde_json::Value>,
    request_id: RequestId,
) -> Result<AgentConflictsParams, Response> {
    let Some(params) = params else {
        return Err(invalid_params_response(
            request_id,
            "agent.conflicts requires params".to_string(),
        ));
    };

    serde_json::from_value::<AgentConflictsParams>(params).map_err(|error| {
        invalid_params_response(
            request_id,
            format!("failed to decode agent.conflicts params: {error}"),
        )
    })
}

fn handle_agent_list(request: Request, state: &RpcServerState) -> Response {
    let params = match parse_agent_list_params(request.params, request.id.clone()) {
        Ok(params) => params,
        Err(response) => return response,
    };

    match state.agent_list(params.workspace_id) {
        Ok(result) => Response::success(request.id, json!(result)),
        Err(reason) => Response::error(
            request.id,
            RpcError {
                code: INTERNAL_ERROR,
                message: format!("failed to list agents: {reason}"),
                data: None,
            },
        ),
    }
}

fn parse_agent_list_params(
    params: Option<serde_json::Value>,
    request_id: RequestId,
) -> Result<AgentListParams, Response> {
    let Some(params) = params else {
        return Err(invalid_params_response(request_id, "agent.list requires params".to_string()));
    };

    serde_json::from_value::<AgentListParams>(params).map_err(|error| {
        invalid_params_response(request_id, format!("failed to decode agent.list params: {error}"))
    })
}

fn handle_agent_claim(request: Request, state: &RpcServerState) -> Response {
    let params = match parse_agent_claim_params(request.params, request.id.clone()) {
        Ok(params) => params,
        Err(response) => return response,
    };

    match state.agent_claim(
        params.workspace_id,
        params.doc_id,
        params.section_id,
        params.ttl_sec,
        params.mode,
        params.note,
        params.agent_id,
    ) {
        Ok(result) => Response::success(request.id, json!(result)),
        Err(reason) => {
            if reason.contains("ttl_sec must be > 0")
                || reason.contains("section_id must not be empty")
            {
                invalid_params_response(request.id, reason)
            } else {
                Response::error(
                    request.id,
                    RpcError {
                        code: INTERNAL_ERROR,
                        message: format!("failed to claim section: {reason}"),
                        data: None,
                    },
                )
            }
        }
    }
}

fn parse_agent_claim_params(
    params: Option<serde_json::Value>,
    request_id: RequestId,
) -> Result<AgentClaimParams, Response> {
    let Some(params) = params else {
        return Err(invalid_params_response(request_id, "agent.claim requires params".to_string()));
    };

    serde_json::from_value::<AgentClaimParams>(params).map_err(|error| {
        invalid_params_response(request_id, format!("failed to decode agent.claim params: {error}"))
    })
}

fn extract_section_content(markdown: &str, section: Option<&Section>) -> String {
    let Some(section) = section else {
        return markdown.to_string();
    };

    let start_line = section.start_line.saturating_sub(1) as usize;
    let end_line = section.end_line.saturating_sub(1) as usize;

    markdown
        .lines()
        .enumerate()
        .filter_map(|(index, line)| (index >= start_line && index < end_line).then_some(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn section_parent_chain(
    section: &Section,
    sections_by_id: &HashMap<String, Section>,
) -> Vec<Section> {
    let mut parents = Vec::new();
    let mut current_parent = section.parent_id.clone();

    while let Some(parent_id) = current_parent {
        let Some(parent) = sections_by_id.get(&parent_id) else {
            break;
        };
        parents.push(parent.clone());
        current_parent = parent.parent_id.clone();
    }

    parents.reverse();
    parents
}

fn section_descendants(
    section: &Section,
    sections: &[Section],
    sections_by_id: &HashMap<String, Section>,
) -> Vec<Section> {
    sections
        .iter()
        .filter(|candidate| candidate.id != section.id)
        .filter(|candidate| section_is_descendant_of(candidate, &section.id, sections_by_id))
        .cloned()
        .collect()
}

fn section_is_descendant_of(
    candidate: &Section,
    ancestor_id: &str,
    sections_by_id: &HashMap<String, Section>,
) -> bool {
    let mut current_parent = candidate.parent_id.as_deref();
    while let Some(parent_id) = current_parent {
        if parent_id == ancestor_id {
            return true;
        }
        current_parent =
            sections_by_id.get(parent_id).and_then(|section| section.parent_id.as_deref());
    }
    false
}

fn comment_matches_target_section(
    comment_section_id: Option<&str>,
    target_section: Option<&Section>,
    sections_by_id: &HashMap<String, Section>,
) -> bool {
    let Some(target_section) = target_section else {
        return true;
    };
    let Some(comment_section_id) = comment_section_id else {
        // Document-level comments are relevant to all section bundles.
        return true;
    };

    if comment_section_id == target_section.id {
        return true;
    }
    if let Some(comment_section) = sections_by_id.get(comment_section_id) {
        return section_is_descendant_of(comment_section, &target_section.id, sections_by_id);
    }

    // Fall back to string prefix matching when comment section metadata is unavailable.
    comment_section_id.starts_with(&format!("{}/", target_section.id))
}

fn sqlite_table_exists(conn: &rusqlite::Connection, table_name: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(
            SELECT 1
            FROM sqlite_master
            WHERE type = 'table' AND name = ?1
        )",
        rusqlite::params![table_name],
        |row| row.get::<_, i64>(0),
    )
    .map(|exists| exists != 0)
}

fn apply_bundle_token_budget(
    section_content: &str,
    context: &mut DocBundleContext,
    token_budget: Option<usize>,
) -> Result<usize, String> {
    apply_bundle_token_budget_with(section_content, context, token_budget, &count_tokens_cl100k)
}

fn apply_bundle_token_budget_with<F>(
    section_content: &str,
    context: &mut DocBundleContext,
    token_budget: Option<usize>,
    token_counter: &F,
) -> Result<usize, String>
where
    F: Fn(&str) -> Result<usize, String>,
{
    let Some(token_budget) = token_budget else {
        return count_bundle_tokens_with(section_content, context, token_counter);
    };

    let mut tokens_used = count_bundle_tokens_with(section_content, context, token_counter)?;
    if tokens_used <= token_budget {
        return Ok(tokens_used);
    }

    while tokens_used > token_budget && !context.comments.is_empty() {
        context.comments.pop();
        tokens_used = count_bundle_tokens_with(section_content, context, token_counter)?;
    }

    while tokens_used > token_budget && !context.backlinks.is_empty() {
        context.backlinks.pop();
        tokens_used = count_bundle_tokens_with(section_content, context, token_counter)?;
    }

    while tokens_used > token_budget && !context.children.is_empty() {
        context.children.pop();
        tokens_used = count_bundle_tokens_with(section_content, context, token_counter)?;
    }

    while tokens_used > token_budget && !context.parents.is_empty() {
        context.parents.pop();
        tokens_used = count_bundle_tokens_with(section_content, context, token_counter)?;
    }

    Ok(tokens_used)
}

fn count_bundle_tokens_with<F>(
    section_content: &str,
    context: &DocBundleContext,
    token_counter: &F,
) -> Result<usize, String>
where
    F: Fn(&str) -> Result<usize, String>,
{
    let mut total = token_counter(section_content)?;
    total += count_serialized_tokens_with(&context.parents, token_counter)?;
    total += count_serialized_tokens_with(&context.children, token_counter)?;
    total += count_serialized_tokens_with(&context.backlinks, token_counter)?;
    total += count_serialized_tokens_with(&context.comments, token_counter)?;
    Ok(total)
}

fn count_serialized_tokens_with<T, F>(value: &T, token_counter: &F) -> Result<usize, String>
where
    T: Serialize,
    F: Fn(&str) -> Result<usize, String>,
{
    let serialized = serde_json::to_string(value)
        .map_err(|error| format!("failed to serialize bundle data: {error}"))?;
    token_counter(&serialized)
}

fn count_tokens_cl100k(value: &str) -> Result<usize, String> {
    let tokenizer = cl100k_tokenizer()?;
    Ok(tokenizer.encode_with_special_tokens(value).len())
}

fn cl100k_tokenizer() -> Result<&'static CoreBPE, String> {
    static TOKENIZER: OnceLock<Result<CoreBPE, String>> = OnceLock::new();
    let tokenizer =
        TOKENIZER.get_or_init(|| tiktoken_rs::cl100k_base().map_err(|error| error.to_string()));

    match tokenizer {
        Ok(tokenizer) => Ok(tokenizer),
        Err(error) => Err(error.clone()),
    }
}

fn decode_doc_edit_ops(value: &serde_json::Value) -> Result<Vec<u8>, String> {
    match value {
        serde_json::Value::String(payload_b64) => decode_doc_edit_ops_base64(payload_b64),
        serde_json::Value::Array(bytes) => decode_doc_edit_ops_array(bytes),
        serde_json::Value::Object(object) => {
            let Some(payload_b64_value) =
                object.get("payload_b64").or_else(|| object.get("base64"))
            else {
                return Err(
                    "doc.edit `ops` object must include `payload_b64` (or `base64`)".to_string()
                );
            };
            let Some(payload_b64) = payload_b64_value.as_str() else {
                return Err("doc.edit `ops.payload_b64` must be a base64 string".to_string());
            };
            decode_doc_edit_ops_base64(payload_b64)
        }
        _ => {
            Err("doc.edit `ops` must be a base64 string, byte array, or object with `payload_b64`"
                .to_string())
        }
    }
}

fn decode_doc_edit_ops_base64(payload_b64: &str) -> Result<Vec<u8>, String> {
    if payload_b64.trim().is_empty() {
        return Err("doc.edit `ops` base64 payload must not be empty".to_string());
    }
    base64::engine::general_purpose::STANDARD
        .decode(payload_b64)
        .map_err(|error| format!("doc.edit `ops` base64 decode failed: {error}"))
}

fn decode_doc_edit_ops_array(values: &[serde_json::Value]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(values.len());
    for (index, value) in values.iter().enumerate() {
        let Some(number) = value.as_u64() else {
            return Err(format!("doc.edit `ops[{index}]` must be an integer byte (0-255)"));
        };
        if number > u8::MAX as u64 {
            return Err(format!("doc.edit `ops[{index}]` out of range: {number}"));
        }
        bytes.push(number as u8);
    }
    if bytes.is_empty() {
        return Err("doc.edit `ops` byte array must not be empty".to_string());
    }
    Ok(bytes)
}

fn decode_doc_search_cursor(cursor: Option<&str>) -> Result<usize, String> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    if cursor.trim().is_empty() {
        return Err("cursor must not be empty".to_string());
    }

    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|error| format!("invalid cursor encoding: {error}"))?;
    let as_text = String::from_utf8(decoded)
        .map_err(|error| format!("cursor is not valid utf-8: {error}"))?;
    as_text.parse::<usize>().map_err(|error| format!("cursor is not a valid offset: {error}"))
}

fn encode_doc_search_cursor(offset: usize) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(offset.to_string())
}

fn resolve_doc_history_range(
    head_seq: i64,
    from_seq: Option<i64>,
    to_seq: Option<i64>,
) -> Result<(i64, i64), String> {
    let from_seq = from_seq.unwrap_or(0);
    let to_seq = to_seq.unwrap_or(head_seq);
    if from_seq < 0 || to_seq < 0 {
        return Err("from_seq and to_seq must be >= 0".to_string());
    }
    if from_seq > to_seq {
        return Err(format!("from_seq must be <= to_seq (from_seq={from_seq}, to_seq={to_seq})"));
    }
    if from_seq > head_seq || to_seq > head_seq {
        return Err(format!(
            "requested sequence range [{from_seq}, {to_seq}] exceeds head_seq {head_seq}"
        ));
    }
    Ok((from_seq, to_seq))
}

fn snapshot_to_diff_snapshot(seq: i64, snapshot: &DocSnapshotRecord) -> DocDiffSnapshot {
    DocDiffSnapshot {
        seq,
        timestamp: snapshot.timestamp,
        content_md: snapshot.content_md.clone(),
        author_attributions: vec![DocDiffSnapshotAttribution {
            author_id: snapshot.author_id.clone(),
            author_type: snapshot.author_type,
            timestamp: snapshot.timestamp,
        }],
        authorship_segments: snapshot_authorship_segments(snapshot),
    }
}

fn snapshot_authorship_segments(snapshot: &DocSnapshotRecord) -> Vec<DocDiffAuthorshipSegment> {
    let content_len = snapshot.content_md.chars().count();
    if content_len == 0 {
        return Vec::new();
    }
    vec![DocDiffAuthorshipSegment {
        author_id: snapshot.author_id.clone(),
        author_type: snapshot.author_type,
        start_offset: 0,
        end_offset: content_len,
        timestamp: snapshot.timestamp,
    }]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkdownLineEdit<'a> {
    Equal(&'a str),
    Delete(&'a str),
    Insert(&'a str),
}

fn render_markdown_patch(from_content: &str, to_content: &str) -> String {
    if from_content == to_content {
        return String::new();
    }

    let from_lines = split_markdown_lines(from_content);
    let to_lines = split_markdown_lines(to_content);
    let edits = diff_markdown_lines(&from_lines, &to_lines);

    let mut out = String::from("```diff\n");
    for edit in edits {
        match edit {
            MarkdownLineEdit::Equal(line) => {
                out.push(' ');
                out.push_str(line);
                out.push('\n');
            }
            MarkdownLineEdit::Delete(line) => {
                out.push('-');
                out.push_str(line);
                out.push('\n');
            }
            MarkdownLineEdit::Insert(line) => {
                out.push('+');
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out.push_str("```");
    out
}

fn split_markdown_lines(value: &str) -> Vec<&str> {
    if value.is_empty() {
        Vec::new()
    } else {
        value.split('\n').collect()
    }
}

fn diff_markdown_lines<'a>(
    from_lines: &'a [&'a str],
    to_lines: &'a [&'a str],
) -> Vec<MarkdownLineEdit<'a>> {
    const MAX_LCS_CELLS: usize = 4_000_000;
    let n = from_lines.len();
    let m = to_lines.len();

    // Bound memory for degenerate large docs; fallback keeps output valid.
    if n.saturating_mul(m) > MAX_LCS_CELLS {
        let mut edits = Vec::with_capacity(n + m);
        edits.extend(from_lines.iter().copied().map(MarkdownLineEdit::Delete));
        edits.extend(to_lines.iter().copied().map(MarkdownLineEdit::Insert));
        return edits;
    }

    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if from_lines[i] == to_lines[j] {
                lcs[i + 1][j + 1].saturating_add(1)
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut edits = Vec::with_capacity(n.saturating_add(m));
    let mut i = 0usize;
    let mut j = 0usize;
    while i < n && j < m {
        if from_lines[i] == to_lines[j] {
            edits.push(MarkdownLineEdit::Equal(from_lines[i]));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            edits.push(MarkdownLineEdit::Delete(from_lines[i]));
            i += 1;
        } else {
            edits.push(MarkdownLineEdit::Insert(to_lines[j]));
            j += 1;
        }
    }
    while i < n {
        edits.push(MarkdownLineEdit::Delete(from_lines[i]));
        i += 1;
    }
    while j < m {
        edits.push(MarkdownLineEdit::Insert(to_lines[j]));
        j += 1;
    }
    edits
}

fn invalid_params_response(request_id: RequestId, reason: String) -> Response {
    Response::error(
        request_id,
        RpcError {
            code: INVALID_PARAMS,
            message: "Invalid params".to_string(),
            data: Some(json!({ "reason": reason })),
        },
    )
}

// ── Git RPC types ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
struct GitStatusInfo {
    dirty: bool,
    status_output: String,
    policy: GitSyncPolicy,
    ai_configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_sync_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GitSyncAction {
    Commit {
        message: String,
        #[serde(default)]
        trigger_type: Option<String>,
    },
    CommitAndPush {
        message: String,
        #[serde(default)]
        trigger_type: Option<String>,
    },
}

impl GitSyncAction {
    fn with_trigger_type(self, trigger_type: impl Into<String>) -> Self {
        let trigger_type = Some(trigger_type.into());
        match self {
            GitSyncAction::Commit { message, .. } => {
                GitSyncAction::Commit { message, trigger_type }
            }
            GitSyncAction::CommitAndPush { message, .. } => {
                GitSyncAction::CommitAndPush { message, trigger_type }
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct GitSyncParams {
    action: GitSyncAction,
}

#[derive(Debug, Clone, Deserialize)]
struct GitConfigureParams {
    policy: GitSyncPolicy,
}

// ── Workspace RPC handlers ──────────────────────────────────────────

async fn handle_workspace_list(request: Request, state: &RpcServerState) -> Response {
    let params: WorkspaceListParams = match request.params {
        Some(p) => match serde_json::from_value(p) {
            Ok(p) => p,
            Err(e) => {
                return invalid_params_response(
                    request.id,
                    format!("failed to decode workspace.list params: {e}"),
                );
            }
        },
        None => WorkspaceListParams { offset: 0, limit: default_workspace_list_limit() },
    };

    let result = state.workspace_list(params.offset, params.limit).await;
    Response::success(request.id, json!(result))
}

async fn handle_workspace_open(request: Request, state: &RpcServerState) -> Response {
    let Some(params) = request.params else {
        return invalid_params_response(request.id, "workspace.open requires params".to_string());
    };

    let params: WorkspaceOpenParams = match serde_json::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return invalid_params_response(
                request.id,
                format!("failed to decode workspace.open params: {e}"),
            );
        }
    };

    match state.workspace_open(params).await {
        Ok(result) => Response::success(request.id, json!(result)),
        Err(reason) => {
            if reason.contains("requires workspace_id or root_path")
                || reason.contains("either workspace_id or root_path")
                || reason.contains("must not be empty")
                || reason.contains("must be an absolute path")
            {
                invalid_params_response(request.id, reason)
            } else {
                Response::error(
                    request.id,
                    RpcError { code: INTERNAL_ERROR, message: reason, data: None },
                )
            }
        }
    }
}

async fn handle_workspace_create(request: Request, state: &RpcServerState) -> Response {
    let Some(params) = request.params else {
        return invalid_params_response(request.id, "workspace.create requires params".to_string());
    };

    let params: WorkspaceCreateParams = match serde_json::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return invalid_params_response(
                request.id,
                format!("failed to decode workspace.create params: {e}"),
            );
        }
    };

    match state.workspace_create(params.name, params.root_path).await {
        Ok(result) => Response::success(request.id, json!(result)),
        Err(reason) => {
            if reason.contains("must not be empty")
                || reason.contains("must be an absolute path")
                || reason.contains("path collision after normalization")
                || reason.contains("invalid markdown path")
            {
                invalid_params_response(request.id, reason)
            } else {
                Response::error(
                    request.id,
                    RpcError { code: INTERNAL_ERROR, message: reason, data: None },
                )
            }
        }
    }
}

// ── Git RPC handlers ────────────────────────────────────────────────

fn handle_git_status(request: Request, state: &RpcServerState) -> Response {
    let Some(git) = &state.git_state else {
        return Response::error(
            request.id,
            RpcError {
                code: INTERNAL_ERROR,
                message: "git not configured".to_string(),
                data: None,
            },
        );
    };

    match git.status_info() {
        Ok(info) => Response::success(request.id, json!(info)),
        Err(e) => Response::error(
            request.id,
            RpcError {
                code: INTERNAL_ERROR,
                message: format!("git status failed: {e}"),
                data: None,
            },
        ),
    }
}

async fn handle_git_sync(request: Request, state: &RpcServerState) -> Response {
    let Some(git) = &state.git_state else {
        return Response::error(
            request.id,
            RpcError {
                code: INTERNAL_ERROR,
                message: "git not configured".to_string(),
                data: None,
            },
        );
    };

    let Some(params) = request.params else {
        return invalid_params_response(request.id, "git.sync requires params".to_string());
    };

    let params: GitSyncParams = match serde_json::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return invalid_params_response(
                request.id,
                format!("failed to decode git.sync params: {e}"),
            );
        }
    };

    let checkpoint_message = match &params.action {
        GitSyncAction::Commit { message, .. } => message.clone(),
        GitSyncAction::CommitAndPush { message, .. } => message.clone(),
    };
    state.enqueue_git_trigger(TriggerEvent::ExplicitCheckpoint {
        agent: state.agent_id.as_ref().clone(),
        message: Some(checkpoint_message),
    });

    let action = params.action.with_trigger_type("checkpoint");
    match git.sync(action).await {
        Ok(job_id) => {
            state.clear_trigger_state_after_commit();
            Response::success(request.id, json!({ "job_id": job_id }))
        }
        Err(e) => Response::error(
            request.id,
            RpcError { code: INTERNAL_ERROR, message: format!("git sync failed: {e}"), data: None },
        ),
    }
}

fn handle_git_configure(request: Request, state: &RpcServerState) -> Response {
    let Some(git) = &state.git_state else {
        return Response::error(
            request.id,
            RpcError {
                code: INTERNAL_ERROR,
                message: "git not configured".to_string(),
                data: None,
            },
        );
    };

    let Some(params) = request.params else {
        return invalid_params_response(request.id, "git.configure requires params".to_string());
    };

    let params: GitConfigureParams = match serde_json::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return invalid_params_response(
                request.id,
                format!("failed to decode git.configure params: {e}"),
            );
        }
    };

    git.set_policy(params.policy.clone());
    Response::success(request.id, json!({ "policy": params.policy }))
}

fn default_metadata(workspace_id: Uuid, doc_id: Uuid) -> DocMetadataRecord {
    DocMetadataRecord {
        workspace_id,
        doc_id,
        path: format!("{doc_id}.md"),
        title: "Untitled".to_string(),
        head_seq: 0,
        etag: format!("doc:{doc_id}:0"),
    }
}

fn metadata_to_rpc_document(metadata: &DocMetadataRecord) -> RpcDocument {
    let created_at =
        chrono::DateTime::<chrono::Utc>::from_timestamp_millis(0).expect("unix epoch should fit");
    let updated_at =
        chrono::DateTime::<chrono::Utc>::from_timestamp_millis(metadata.head_seq.max(0))
            .unwrap_or(created_at);

    RpcDocument {
        id: metadata.doc_id,
        workspace_id: metadata.workspace_id,
        path: metadata.path.clone(),
        title: metadata.title.clone(),
        tags: Vec::new(),
        head_seq: metadata.head_seq,
        etag: metadata.etag.clone(),
        archived_at: None,
        deleted_at: None,
        created_at,
        updated_at,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::collections::VecDeque;
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use base64::Engine;
    use chrono::Utc;
    use scriptum_common::protocol::jsonrpc::{Request, RequestId, INTERNAL_ERROR, INVALID_PARAMS};
    use scriptum_common::types::Section;
    use serde_json::json;
    use tokio::sync::broadcast;
    use uuid::Uuid;

    use crate::engine::ydoc::YDoc;
    use crate::git::commit::{AiCommitClient, AiCommitError, RedactionPolicy as AiRedactionPolicy};
    use crate::git::worker::{CommandExecutor, CommandResult};
    use crate::search::{BacklinkStore, ResolvedBacklink};

    use super::{
        apply_bundle_token_budget_with, dispatch_request, BacklinkContext, CommentThreadContext,
        DocBundleContext, GitOps, GitState, GitStatusInfo, GitSyncAction, GitSyncPolicy,
        RpcServerState, TriggerConfig,
    };

    // ── Mock GitOps ────────────────────────────────────────────────────

    #[derive(Clone)]
    struct MockGitOps {
        status_result: Arc<Mutex<Result<GitStatusInfo, String>>>,
        sync_result: Arc<Mutex<Result<Uuid, String>>>,
        policy: Arc<Mutex<GitSyncPolicy>>,
        last_sync: Arc<Mutex<Option<chrono::DateTime<chrono::Utc>>>>,
        sync_calls: Arc<Mutex<Vec<String>>>,
    }

    impl MockGitOps {
        fn new() -> Self {
            Self {
                status_result: Arc::new(Mutex::new(Ok(GitStatusInfo {
                    dirty: false,
                    status_output: String::new(),
                    policy: GitSyncPolicy::Manual,
                    ai_configured: false,
                    last_sync_at: None,
                }))),
                sync_result: Arc::new(Mutex::new(Ok(Uuid::nil()))),
                policy: Arc::new(Mutex::new(GitSyncPolicy::Manual)),
                last_sync: Arc::new(Mutex::new(None)),
                sync_calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn with_status(self, result: Result<GitStatusInfo, String>) -> Self {
            *self.status_result.lock().unwrap() = result;
            self
        }

        fn with_sync_result(self, result: Result<Uuid, String>) -> Self {
            *self.sync_result.lock().unwrap() = result;
            self
        }
    }

    impl GitOps for MockGitOps {
        fn status_info(&self) -> Result<GitStatusInfo, String> {
            self.status_result.lock().unwrap().clone()
        }

        fn sync(
            &self,
            action: GitSyncAction,
        ) -> Pin<Box<dyn Future<Output = Result<Uuid, String>> + Send + '_>> {
            let label = match &action {
                GitSyncAction::Commit { message, trigger_type } => match trigger_type {
                    Some(trigger) => format!("commit:{message}|trigger:{trigger}"),
                    None => format!("commit:{message}"),
                },
                GitSyncAction::CommitAndPush { message, trigger_type } => match trigger_type {
                    Some(trigger) => format!("commit_and_push:{message}|trigger:{trigger}"),
                    None => format!("commit_and_push:{message}"),
                },
            };
            self.sync_calls.lock().unwrap().push(label);
            let result = self.sync_result.lock().unwrap().clone();
            Box::pin(async move { result })
        }

        fn get_policy(&self) -> GitSyncPolicy {
            self.policy.lock().unwrap().clone()
        }

        fn set_policy(&self, policy: GitSyncPolicy) {
            *self.policy.lock().unwrap() = policy;
        }

        fn last_sync_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
            *self.last_sync.lock().unwrap()
        }

        fn mark_synced(&self) {
            *self.last_sync.lock().unwrap() = Some(chrono::Utc::now());
        }
    }

    fn state_with_git(mock: MockGitOps) -> RpcServerState {
        let mut state = RpcServerState::default();
        state.git_state = Some(Arc::new(mock));
        state
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct GitInvocation {
        program: String,
        args: Vec<String>,
        cwd: PathBuf,
    }

    #[derive(Clone)]
    struct MockCommandExecutor {
        calls: Arc<Mutex<Vec<GitInvocation>>>,
        responses: Arc<Mutex<VecDeque<Result<CommandResult, std::io::Error>>>>,
    }

    impl MockCommandExecutor {
        fn new(responses: Vec<Result<CommandResult, std::io::Error>>) -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                responses: Arc::new(Mutex::new(VecDeque::from(responses))),
            }
        }

        fn calls(&self) -> Vec<GitInvocation> {
            self.calls.lock().expect("mock calls lock poisoned").clone()
        }
    }

    impl CommandExecutor for MockCommandExecutor {
        fn execute(
            &self,
            program: &str,
            args: &[String],
            cwd: &Path,
        ) -> Result<CommandResult, std::io::Error> {
            self.calls.lock().expect("mock calls lock poisoned").push(GitInvocation {
                program: program.to_string(),
                args: args.to_vec(),
                cwd: cwd.to_path_buf(),
            });

            self.responses
                .lock()
                .expect("mock responses lock poisoned")
                .pop_front()
                .expect("missing mock response")
        }
    }

    struct MockAiClient {
        response: Arc<Mutex<Result<String, AiCommitError>>>,
    }

    impl MockAiClient {
        fn success(message: &str) -> Self {
            Self { response: Arc::new(Mutex::new(Ok(message.to_string()))) }
        }

        fn failure(error: AiCommitError) -> Self {
            Self { response: Arc::new(Mutex::new(Err(error))) }
        }
    }

    impl AiCommitClient for MockAiClient {
        fn generate(
            &self,
            _system: &str,
            _user_prompt: &str,
        ) -> Pin<Box<dyn Future<Output = Result<String, AiCommitError>> + Send>> {
            let response = self.response.lock().expect("mock ai response lock poisoned").clone();
            Box::pin(async move { response })
        }
    }

    fn encoded_doc_ops(content: &str) -> String {
        let source = YDoc::new();
        source.insert_text("content", 0, content);
        base64::engine::general_purpose::STANDARD.encode(source.encode_state())
    }

    fn ok_command(stdout: &str) -> Result<CommandResult, std::io::Error> {
        Ok(CommandResult {
            success: true,
            code: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
        })
    }

    #[tokio::test]
    async fn doc_read_returns_content_and_sections() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        let markdown = "# Root\n\n## Child\n";
        state.seed_doc(workspace_id, doc_id, "docs/readme.md", "Readme", markdown).await;

        let request = Request::new(
            "doc.read",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "include_content": true
            })),
            RequestId::Number(1),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success response: {response:?}");
        let result = response.result.expect("result should be populated");
        assert_eq!(result["document"]["workspace_id"], json!(workspace_id));
        assert_eq!(result["document"]["id"], json!(doc_id));
        assert_eq!(result["document"]["path"], json!("docs/readme.md"));
        assert_eq!(result["document"]["title"], json!("Readme"));
        assert_eq!(result["attributions"], json!([]));
        assert_eq!(result["content_md"], json!(markdown));
        assert_eq!(result["sections"].as_array().expect("sections should be an array").len(), 2);
    }

    #[tokio::test]
    async fn doc_read_omits_content_when_include_content_is_false() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        state.seed_doc(workspace_id, doc_id, "docs/note.md", "Note", "# Heading\n\nBody").await;

        let request = Request::new(
            "doc.read",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "include_content": false
            })),
            RequestId::Number(2),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success response: {response:?}");
        let result = response.result.expect("result should be populated");
        assert_eq!(result.get("content_md"), None);
        assert_eq!(result.get("backlinks"), None);
        assert_eq!(result["sections"].as_array().expect("sections should be an array").len(), 1);
    }

    #[tokio::test]
    async fn doc_read_rejects_invalid_params() {
        let state = RpcServerState::default();
        let request = Request::new(
            "doc.read",
            Some(json!({
                "workspace_id": Uuid::new_v4(),
                // missing doc_id
                "include_content": true
            })),
            RequestId::Number(3),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.result.is_none());
        let error = response.error.expect("error should be present");
        assert_eq!(error.code, -32602);
    }

    #[tokio::test]
    async fn doc_edit_replaces_content_and_advances_head_seq() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        state.seed_doc(workspace_id, doc_id, "docs/spec.md", "Spec", "# Before\nold").await;

        let request = Request::new(
            "doc.edit",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "client_update_id": "upd-1",
                "content_md": "# After\nnew",
                "if_etag": format!("doc:{doc_id}:0"),
                "agent_id": "cursor-1"
            })),
            RequestId::Number(80),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success response: {response:?}");
        let result = response.result.expect("result should be populated");
        assert_eq!(result["head_seq"], json!(1));
        assert_eq!(result["etag"], json!(format!("doc:{doc_id}:1")));

        let read = state.read_doc(workspace_id, doc_id, true, false).await;
        assert_eq!(read.content_md, Some("# After\nnew".to_string()));
        assert_eq!(read.document.head_seq, 1);
        assert_eq!(read.document.etag, format!("doc:{doc_id}:1"));
    }

    #[tokio::test]
    async fn doc_edit_applies_yjs_ops_payload() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        state.seed_doc(workspace_id, doc_id, "docs/live.md", "Live", "").await;
        let ops_payload_b64 = encoded_doc_ops("inserted via ops");

        let request = Request::new(
            "doc.edit",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "client_update_id": "upd-ops-1",
                "ops": {
                    "payload_b64": ops_payload_b64
                }
            })),
            RequestId::Number(81),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success response: {response:?}");
        let read = state.read_doc(workspace_id, doc_id, true, false).await;
        assert_eq!(read.content_md, Some("inserted via ops".to_string()));
        assert_eq!(read.document.head_seq, 1);
    }

    #[tokio::test]
    async fn doc_edit_indexes_backlinks_and_doc_read_returns_incoming_backlinks() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let target_doc_id = Uuid::new_v4();
        let source_doc_id = Uuid::new_v4();
        state.seed_doc(workspace_id, target_doc_id, "docs/target.md", "Target", "# Target\n").await;
        state.seed_doc(workspace_id, source_doc_id, "docs/source.md", "Source", "# Source\n").await;

        let edit_response = dispatch_request(
            Request::new(
                "doc.edit",
                Some(json!({
                    "workspace_id": workspace_id,
                    "doc_id": source_doc_id,
                    "client_update_id": "upd-backlinks-add-1",
                    "content_md": "# Source\nSee [[docs/target.md]].",
                })),
                RequestId::Number(84),
            ),
            &state,
        )
        .await;
        assert!(edit_response.error.is_none(), "doc.edit should succeed: {edit_response:?}");

        let read_response = dispatch_request(
            Request::new(
                "doc.read",
                Some(json!({
                    "workspace_id": workspace_id,
                    "doc_id": target_doc_id,
                    "include_backlinks": true
                })),
                RequestId::Number(85),
            ),
            &state,
        )
        .await;
        assert!(read_response.error.is_none(), "doc.read should succeed: {read_response:?}");
        let read_result = read_response.result.expect("doc.read result should be present");
        let backlinks = read_result["backlinks"]
            .as_array()
            .expect("doc.read backlinks should be present when include_backlinks=true");
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0]["doc_id"], json!(source_doc_id));
        assert_eq!(backlinks[0]["path"], json!("docs/source.md"));
        assert_eq!(backlinks[0]["snippet"], json!("docs/target.md"));

        let bundle_response = dispatch_request(
            Request::new(
                "doc.bundle",
                Some(json!({
                    "workspace_id": workspace_id,
                    "doc_id": target_doc_id,
                    "include": ["backlinks"],
                    "token_budget": 4000
                })),
                RequestId::Number(86),
            ),
            &state,
        )
        .await;
        assert!(bundle_response.error.is_none(), "doc.bundle should succeed: {bundle_response:?}");
        let bundle_result = bundle_response.result.expect("doc.bundle result should be present");
        assert_eq!(bundle_result["context"]["backlinks"][0]["doc_id"], json!(source_doc_id));
        assert_eq!(bundle_result["context"]["backlinks"][0]["path"], json!("docs/source.md"));
    }

    #[tokio::test]
    async fn doc_edit_removes_backlinks_when_links_are_deleted() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let target_doc_id = Uuid::new_v4();
        let source_doc_id = Uuid::new_v4();
        state.seed_doc(workspace_id, target_doc_id, "docs/target.md", "Target", "# Target\n").await;
        state.seed_doc(workspace_id, source_doc_id, "docs/source.md", "Source", "# Source\n").await;

        let add_response = dispatch_request(
            Request::new(
                "doc.edit",
                Some(json!({
                    "workspace_id": workspace_id,
                    "doc_id": source_doc_id,
                    "client_update_id": "upd-backlinks-remove-1",
                    "content_md": "# Source\nSee [[docs/target.md]].",
                })),
                RequestId::Number(87),
            ),
            &state,
        )
        .await;
        assert!(add_response.error.is_none(), "initial doc.edit should succeed: {add_response:?}");

        let remove_response = dispatch_request(
            Request::new(
                "doc.edit",
                Some(json!({
                    "workspace_id": workspace_id,
                    "doc_id": source_doc_id,
                    "client_update_id": "upd-backlinks-remove-2",
                    "content_md": "# Source\nNo links now.",
                })),
                RequestId::Number(88),
            ),
            &state,
        )
        .await;
        assert!(
            remove_response.error.is_none(),
            "second doc.edit should succeed: {remove_response:?}"
        );

        let read_response = dispatch_request(
            Request::new(
                "doc.read",
                Some(json!({
                    "workspace_id": workspace_id,
                    "doc_id": target_doc_id,
                    "include_backlinks": true
                })),
                RequestId::Number(89),
            ),
            &state,
        )
        .await;
        assert!(read_response.error.is_none(), "doc.read should succeed: {read_response:?}");
        let read_result = read_response.result.expect("doc.read result should be present");
        assert_eq!(read_result["backlinks"], json!([]));

        let backlink_count = state
            .with_agent_storage(|conn, _| {
                conn.query_row(
                    "SELECT COUNT(*) FROM backlinks WHERE source_doc_id = ?1 AND target_doc_id = ?2",
                    rusqlite::params![source_doc_id.to_string(), target_doc_id.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(|error| format!("failed to count backlinks: {error}"))
            })
            .expect("backlink count should query");
        assert_eq!(backlink_count, 0);
    }

    #[tokio::test]
    async fn doc_edit_writes_wal_and_recovery_restores_content_after_restart() {
        let tmp = tempfile::tempdir().expect("tempdir should be created");
        let crdt_store_dir = tmp.path().join("crdt_store");
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();

        let state = RpcServerState::default().with_crdt_store_dir(crdt_store_dir.clone());
        let edit_response = dispatch_request(
            Request::new(
                "doc.edit",
                Some(json!({
                    "workspace_id": workspace_id,
                    "doc_id": doc_id,
                    "client_update_id": "upd-recover-1",
                    "content_md": "# Recovered\nfrom wal\n",
                })),
                RequestId::Number(815),
            ),
            &state,
        )
        .await;
        assert!(edit_response.error.is_none(), "doc.edit should succeed: {edit_response:?}");

        let wal_path =
            crdt_store_dir.join("wal").join(workspace_id.to_string()).join(format!("{doc_id}.wal"));
        assert!(wal_path.exists(), "doc.edit should create a WAL file");

        // Simulate daemon crash/restart by creating a fresh state and recovering from the same store.
        let recovered_state = RpcServerState::default().with_crdt_store_dir(crdt_store_dir.clone());
        let report = recovered_state
            .recover_docs_at_startup(&crdt_store_dir)
            .await
            .expect("startup recovery should succeed");
        assert_eq!(report.recovered_docs, 1);
        assert!(report.degraded_docs.is_empty());

        let recovered_doc = recovered_state.read_doc(workspace_id, doc_id, true, false).await;
        assert_eq!(recovered_doc.content_md, Some("# Recovered\nfrom wal\n".to_string()));
    }

    #[tokio::test]
    async fn doc_edit_rejects_if_etag_mismatch_without_mutating_doc() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        state.seed_doc(workspace_id, doc_id, "docs/etag.md", "Etag", "unchanged").await;

        let request = Request::new(
            "doc.edit",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "client_update_id": "upd-etag-1",
                "content_md": "mutated",
                "if_etag": format!("doc:{doc_id}:999")
            })),
            RequestId::Number(82),
        );
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INVALID_PARAMS);
        let reason = error
            .data
            .as_ref()
            .and_then(|value| value.get("reason"))
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        assert!(reason.contains("if_etag mismatch"));

        let read = state.read_doc(workspace_id, doc_id, true, false).await;
        assert_eq!(read.content_md, Some("unchanged".to_string()));
        assert_eq!(read.document.head_seq, 0);
        assert_eq!(read.document.etag, format!("doc:{doc_id}:0"));
    }

    #[tokio::test]
    async fn doc_edit_rejects_requests_without_ops_or_content() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        state.seed_doc(workspace_id, doc_id, "docs/empty.md", "Empty", "seed").await;

        let request = Request::new(
            "doc.edit",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "client_update_id": "upd-empty-1"
            })),
            RequestId::Number(83),
        );
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INVALID_PARAMS);
        let reason = error
            .data
            .as_ref()
            .and_then(|value| value.get("reason"))
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        assert!(reason.contains("requires either `ops` or `content_md`"));
    }

    #[tokio::test]
    async fn doc_diff_returns_markdown_patch_between_sequence_points() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        state
            .seed_doc(workspace_id, doc_id, "docs/history.md", "History", "# Title\nold line\n")
            .await;

        let edit_1 = Request::new(
            "doc.edit",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "client_update_id": "upd-diff-1",
                "content_md": "# Title\nnew line\n"
            })),
            RequestId::Number(90),
        );
        let edit_1_response = dispatch_request(edit_1, &state).await;
        assert!(
            edit_1_response.error.is_none(),
            "expected first edit to succeed: {edit_1_response:?}"
        );

        let edit_2 = Request::new(
            "doc.edit",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "client_update_id": "upd-diff-2",
                "content_md": "# Title\nnew line\nextra line\n"
            })),
            RequestId::Number(91),
        );
        let edit_2_response = dispatch_request(edit_2, &state).await;
        assert!(
            edit_2_response.error.is_none(),
            "expected second edit to succeed: {edit_2_response:?}"
        );

        let diff_request = Request::new(
            "doc.diff",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "from_seq": 0,
                "to_seq": 2
            })),
            RequestId::Number(92),
        );
        let diff_response = dispatch_request(diff_request, &state).await;
        assert!(diff_response.error.is_none(), "expected doc.diff to succeed: {diff_response:?}");
        let patch = diff_response
            .result
            .as_ref()
            .and_then(|value| value.get("patch_md"))
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let result = diff_response.result.as_ref().expect("doc.diff result should be present");

        assert!(patch.starts_with("```diff\n"));
        assert!(patch.contains("-old line"));
        assert!(patch.contains("+new line"));
        assert!(patch.contains("+extra line"));
        assert_eq!(result["from_seq"], json!(0));
        assert_eq!(result["to_seq"], json!(2));
        assert_eq!(result["granularity"], json!("snapshot"));
        let snapshots = result["snapshots"].as_array().expect("snapshots should be an array");
        assert_eq!(snapshots.len(), 3);
        assert_eq!(snapshots[0]["seq"], json!(0));
        assert_eq!(snapshots[2]["seq"], json!(2));
        assert_eq!(snapshots[2]["author_attributions"][0]["author_id"], json!("local-user"));
        let end_offset = snapshots[2]["authorship_segments"][0]["end_offset"]
            .as_u64()
            .expect("authorship segment should include numeric end_offset");
        assert!(end_offset > 0);
    }

    #[tokio::test]
    async fn doc_diff_defaults_to_full_history_when_bounds_are_omitted() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        state.seed_doc(workspace_id, doc_id, "docs/history.md", "History", "v0").await;

        let edit_response = dispatch_request(
            Request::new(
                "doc.edit",
                Some(json!({
                    "workspace_id": workspace_id,
                    "doc_id": doc_id,
                    "client_update_id": "upd-default-range-1",
                    "content_md": "v1"
                })),
                RequestId::Number(913),
            ),
            &state,
        )
        .await;
        assert!(edit_response.error.is_none(), "doc.edit should succeed: {edit_response:?}");

        let diff_response = dispatch_request(
            Request::new(
                "doc.diff",
                Some(json!({
                    "workspace_id": workspace_id,
                    "doc_id": doc_id
                })),
                RequestId::Number(914),
            ),
            &state,
        )
        .await;
        assert!(diff_response.error.is_none(), "doc.diff should succeed: {diff_response:?}");
        let result = diff_response.result.expect("doc.diff result should be present");
        assert_eq!(result["from_seq"], json!(0));
        assert_eq!(result["to_seq"], json!(1));
    }

    #[tokio::test]
    async fn doc_diff_rejects_ranges_beyond_head_seq() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        state.seed_doc(workspace_id, doc_id, "docs/history.md", "History", "seed").await;

        let request = Request::new(
            "doc.diff",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "from_seq": 0,
                "to_seq": 1
            })),
            RequestId::Number(93),
        );
        let response = dispatch_request(request, &state).await;
        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INVALID_PARAMS);

        let reason = error
            .data
            .as_ref()
            .and_then(|value| value.get("reason"))
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        assert!(reason.contains("head_seq"));
    }

    #[tokio::test]
    async fn doc_history_returns_timeline_events_with_authors_and_summaries() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        state.seed_doc(workspace_id, doc_id, "docs/history.md", "History", "# Title\nseed\n").await;

        let agent_edit = Request::new(
            "doc.edit",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "client_update_id": "upd-history-1",
                "content_md": "# Title\nagent change\n",
                "agent_id": "agent-1"
            })),
            RequestId::Number(94),
        );
        let agent_edit_response = dispatch_request(agent_edit, &state).await;
        assert!(
            agent_edit_response.error.is_none(),
            "expected agent edit to succeed: {agent_edit_response:?}"
        );

        let human_edit = Request::new(
            "doc.edit",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "client_update_id": "upd-history-2",
                "content_md": "# Title\nhuman change\n"
            })),
            RequestId::Number(95),
        );
        let human_edit_response = dispatch_request(human_edit, &state).await;
        assert!(
            human_edit_response.error.is_none(),
            "expected human edit to succeed: {human_edit_response:?}"
        );

        let history_request = Request::new(
            "doc.history",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id
            })),
            RequestId::Number(96),
        );
        let history_response = dispatch_request(history_request, &state).await;
        assert!(
            history_response.error.is_none(),
            "expected doc.history to succeed: {history_response:?}"
        );

        let result = history_response.result.expect("doc.history result should be present");
        let events = result["events"].as_array().expect("doc.history events should be an array");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0]["seq"], json!(0));
        assert_eq!(events[0]["author_id"], json!("system"));
        assert_eq!(events[1]["seq"], json!(1));
        assert_eq!(events[1]["author_id"], json!("agent-1"));
        assert_eq!(events[1]["summary"], json!("upd-history-1"));
        assert_eq!(events[2]["seq"], json!(2));
        assert_eq!(events[2]["author_id"], json!("local-user"));
        assert_eq!(events[2]["summary"], json!("upd-history-2"));
        assert!(
            events[2]["timestamp"].as_str().is_some(),
            "doc.history events should include timestamps"
        );
    }

    #[tokio::test]
    async fn doc_bundle_returns_section_content_and_context() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let target_doc_id = Uuid::new_v4();
        let source_doc_id = Uuid::new_v4();
        let markdown = "# Root\nroot body\n\n## Child\nchild body\n\n### Grandchild\ndeep body\n";
        state.seed_doc(workspace_id, target_doc_id, "docs/readme.md", "Readme", markdown).await;
        state
            .seed_doc(
                workspace_id,
                source_doc_id,
                "docs/reference.md",
                "Reference",
                "# Reference\nSee [[Readme]].\n",
            )
            .await;
        state
            .with_agent_storage(|conn, _| {
                let backlink_store = BacklinkStore::new(conn);
                backlink_store
                    .ensure_schema()
                    .map_err(|error| format!("failed to ensure backlink schema: {error}"))?;
                backlink_store
                    .replace_for_source(
                        &source_doc_id.to_string(),
                        &[ResolvedBacklink {
                            source_doc_id: source_doc_id.to_string(),
                            target_doc_id: target_doc_id.to_string(),
                            link_text: "Readme".to_string(),
                        }],
                    )
                    .map_err(|error| format!("failed to insert test backlink: {error}"))?;

                conn.execute_batch(
                    "CREATE TABLE IF NOT EXISTS comment_threads (
                        id TEXT PRIMARY KEY,
                        workspace_id TEXT NOT NULL,
                        doc_id TEXT NOT NULL,
                        section_id TEXT NULL,
                        status TEXT NOT NULL,
                        version INTEGER NOT NULL,
                        created_at TEXT NOT NULL,
                        resolved_at TEXT NULL
                    );",
                )
                .map_err(|error| format!("failed to ensure comment_threads schema: {error}"))?;
                conn.execute(
                    "INSERT INTO comment_threads
                        (id, workspace_id, doc_id, section_id, status, version, created_at, resolved_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        "thread-1",
                        workspace_id.to_string(),
                        target_doc_id.to_string(),
                        "root/child",
                        "open",
                        1,
                        Utc::now().to_rfc3339(),
                        Option::<String>::None,
                    ],
                )
                .map_err(|error| format!("failed to insert test comment thread: {error}"))?;
                Ok(())
            })
            .expect("test bundle context data should be seeded");

        let request = Request::new(
            "doc.bundle",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": target_doc_id,
                "section_id": "root/child",
                "include": ["parents", "children", "backlinks", "comments"],
                "token_budget": 8000
            })),
            RequestId::Number(4),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success response: {response:?}");
        let result = response.result.expect("result should be populated");
        assert_eq!(result["section_content"], json!("## Child\nchild body\n"));
        assert_eq!(result["context"]["parents"][0]["id"], json!("root"));
        assert_eq!(result["context"]["children"][0]["id"], json!("root/child/grandchild"));
        assert_eq!(result["context"]["backlinks"][0]["doc_id"], json!(source_doc_id));
        assert_eq!(result["context"]["backlinks"][0]["path"], json!("docs/reference.md"));
        assert_eq!(result["context"]["comments"][0]["id"], json!("thread-1"));
        assert_eq!(result["context"]["comments"][0]["section_id"], json!("root/child"));
        assert!(result["tokens_used"].as_u64().expect("tokens_used should be numeric") > 0);
    }

    #[tokio::test]
    async fn doc_bundle_rejects_unknown_section_id() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        state.seed_doc(workspace_id, doc_id, "docs/readme.md", "Readme", "# Root\n").await;

        let request = Request::new(
            "doc.bundle",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "section_id": "missing",
                "include": ["parents"],
                "token_budget": 1000
            })),
            RequestId::Number(5),
        );
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INVALID_PARAMS);
        assert_eq!(error.message, "Invalid params");
        let reason = error
            .data
            .as_ref()
            .and_then(|value| value.get("reason"))
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        assert!(reason.contains("section `missing` not found"));
    }

    #[test]
    fn bundle_budget_truncation_order_is_comments_backlinks_children_parents() {
        fn token_counter(value: &str) -> Result<usize, String> {
            Ok(value.len())
        }

        fn section(id: &str, parent_id: Option<&str>, heading: &str) -> Section {
            Section {
                id: id.to_string(),
                parent_id: parent_id.map(|value| value.to_string()),
                heading: heading.to_string(),
                level: 2,
                start_line: 1,
                end_line: 2,
            }
        }

        let base_context = DocBundleContext {
            parents: vec![section("root", None, &"P".repeat(120))],
            children: vec![section("root/child", Some("root"), &"C".repeat(120))],
            backlinks: vec![BacklinkContext {
                doc_id: Uuid::new_v4(),
                path: "docs/reference.md".to_string(),
                snippet: "B".repeat(120),
            }],
            comments: vec![CommentThreadContext {
                id: "thread-1".to_string(),
                workspace_id: Uuid::new_v4().to_string(),
                doc_id: Uuid::new_v4().to_string(),
                section_id: Some("root/child".to_string()),
                status: "open".to_string(),
                version: 1,
                created_at: "2026-02-09T00:00:00Z".to_string(),
                resolved_at: None,
            }],
        };
        let section_content = "core section body";

        let mut no_comments = base_context.clone();
        no_comments.comments.clear();
        let budget_without_comments =
            apply_bundle_token_budget_with(section_content, &mut no_comments, None, &token_counter)
                .expect("should count tokens without comments");

        let mut drop_comments = base_context.clone();
        let _ = apply_bundle_token_budget_with(
            section_content,
            &mut drop_comments,
            Some(budget_without_comments),
            &token_counter,
        )
        .expect("should truncate comments");
        assert!(drop_comments.comments.is_empty());
        assert_eq!(drop_comments.backlinks.len(), 1);
        assert_eq!(drop_comments.children.len(), 1);
        assert_eq!(drop_comments.parents.len(), 1);

        let mut no_comments_or_backlinks = base_context.clone();
        no_comments_or_backlinks.comments.clear();
        no_comments_or_backlinks.backlinks.clear();
        let budget_without_comments_or_backlinks = apply_bundle_token_budget_with(
            section_content,
            &mut no_comments_or_backlinks,
            None,
            &token_counter,
        )
        .expect("should count tokens without comments/backlinks");

        let mut drop_comments_then_backlinks = base_context.clone();
        let _ = apply_bundle_token_budget_with(
            section_content,
            &mut drop_comments_then_backlinks,
            Some(budget_without_comments_or_backlinks),
            &token_counter,
        )
        .expect("should truncate comments then backlinks");
        assert!(drop_comments_then_backlinks.comments.is_empty());
        assert!(drop_comments_then_backlinks.backlinks.is_empty());
        assert_eq!(drop_comments_then_backlinks.children.len(), 1);
        assert_eq!(drop_comments_then_backlinks.parents.len(), 1);

        let mut only_parents = base_context.clone();
        only_parents.comments.clear();
        only_parents.backlinks.clear();
        only_parents.children.clear();
        let budget_without_comments_backlinks_children = apply_bundle_token_budget_with(
            section_content,
            &mut only_parents,
            None,
            &token_counter,
        )
        .expect("should count tokens with parents only");

        let mut drop_comments_backlinks_children = base_context.clone();
        let _ = apply_bundle_token_budget_with(
            section_content,
            &mut drop_comments_backlinks_children,
            Some(budget_without_comments_backlinks_children),
            &token_counter,
        )
        .expect("should truncate comments/backlinks/children");
        assert!(drop_comments_backlinks_children.comments.is_empty());
        assert!(drop_comments_backlinks_children.backlinks.is_empty());
        assert!(drop_comments_backlinks_children.children.is_empty());
        assert_eq!(drop_comments_backlinks_children.parents.len(), 1);

        let mut section_only_context = DocBundleContext::default();
        let section_only_budget = apply_bundle_token_budget_with(
            section_content,
            &mut section_only_context,
            None,
            &token_counter,
        )
        .expect("should count section-only tokens");

        let mut drop_all_context = base_context.clone();
        let _ = apply_bundle_token_budget_with(
            section_content,
            &mut drop_all_context,
            Some(section_only_budget),
            &token_counter,
        )
        .expect("should drop all context groups");
        assert!(drop_all_context.comments.is_empty());
        assert!(drop_all_context.backlinks.is_empty());
        assert!(drop_all_context.children.is_empty());
        assert!(drop_all_context.parents.is_empty());
    }

    #[tokio::test]
    async fn daemon_shutdown_notifies_runtime_when_configured() {
        let (shutdown_tx, mut shutdown_rx) = broadcast::channel(1);
        let state = RpcServerState::default().with_shutdown_notifier(shutdown_tx);
        let request = Request::new("daemon.shutdown", None, RequestId::Number(4));
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success response: {response:?}");
        assert_eq!(response.result.expect("result should be populated"), json!({ "ok": true }));
        shutdown_rx.recv().await.expect("shutdown notification should be sent");
    }

    // ── agent.* tests ─────────────────────────────────────────────────

    async fn claim_section(
        state: &RpcServerState,
        request_id: i64,
        workspace_id: Uuid,
        doc_id: Uuid,
        section_id: &str,
        agent_id: &str,
        mode: &str,
    ) {
        let request = Request::new(
            "agent.claim",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "section_id": section_id,
                "ttl_sec": 600,
                "mode": mode,
                "agent_id": agent_id
            })),
            RequestId::Number(request_id),
        );
        let response = dispatch_request(request, state).await;
        assert!(response.error.is_none(), "claim should succeed: {response:?}");
    }

    #[tokio::test]
    async fn agent_whoami_returns_id_and_capabilities() {
        let state = RpcServerState::default().with_agent_identity("claude-1");
        let request = Request::new("agent.whoami", Some(json!({})), RequestId::Number(60));
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result should be populated");
        assert_eq!(result["agent_id"], "claude-1");
        let capabilities =
            result["capabilities"].as_array().expect("capabilities should be an array");
        assert!(capabilities.contains(&json!("agent.claim")));
        assert!(capabilities.contains(&json!("agent.status")));
    }

    #[tokio::test]
    async fn agent_claim_returns_lease_and_conflicts() {
        let state = RpcServerState::default().with_agent_identity("claude-1");
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();

        claim_section(&state, 61, workspace_id, doc_id, "root/auth", "claude-1", "exclusive").await;

        let request = Request::new(
            "agent.claim",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "section_id": "root/auth",
                "ttl_sec": 600,
                "mode": "shared",
                "agent_id": "copilot-1"
            })),
            RequestId::Number(62),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result should be populated");
        let lease_id = result["lease_id"].as_str().expect("lease_id should be a string");
        assert!(lease_id.contains(&workspace_id.to_string()));
        assert!(lease_id.contains(&doc_id.to_string()));
        assert!(lease_id.contains("root/auth"));
        assert!(lease_id.contains("copilot-1"));

        let conflicts = result["conflicts"].as_array().expect("conflicts should be an array");
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0]["agent_id"], "claude-1");
        assert_eq!(conflicts[0]["section_id"], "root/auth");
        assert!(result["expires_at"].as_str().is_some());
    }

    #[tokio::test]
    async fn agent_status_returns_active_sessions_with_section_counts() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();

        claim_section(&state, 63, workspace_id, doc_id, "root/auth", "claude-1", "exclusive").await;
        claim_section(&state, 64, workspace_id, doc_id, "root/auth/oauth", "claude-1", "shared")
            .await;
        claim_section(&state, 65, workspace_id, doc_id, "root/api", "copilot-1", "shared").await;

        let request = Request::new(
            "agent.status",
            Some(json!({ "workspace_id": workspace_id })),
            RequestId::Number(66),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result should be populated");
        let change_token =
            result["change_token"].as_str().expect("change_token should be present and a string");
        assert!(!change_token.is_empty(), "change_token should not be empty");
        let sessions =
            result["active_sessions"].as_array().expect("active_sessions should be an array");
        assert_eq!(sessions.len(), 2);

        let mut sections_by_agent = HashMap::new();
        for session in sessions {
            let agent_id =
                session["agent_id"].as_str().expect("agent_id should be a string").to_string();
            let active_sections =
                session["active_sections"].as_u64().expect("active_sections should be numeric")
                    as u32;
            sections_by_agent.insert(agent_id, active_sections);
        }
        assert_eq!(sections_by_agent.get("claude-1"), Some(&2));
        assert_eq!(sections_by_agent.get("copilot-1"), Some(&1));
    }

    #[tokio::test]
    async fn agent_status_change_token_changes_after_doc_edit() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        state.seed_doc(workspace_id, doc_id, "docs/status.md", "Status", "# Before\n").await;

        let status_before = Request::new(
            "agent.status",
            Some(json!({ "workspace_id": workspace_id })),
            RequestId::Number(670),
        );
        let before_response = dispatch_request(status_before, &state).await;
        assert!(before_response.error.is_none(), "expected success: {before_response:?}");
        let before_token = before_response
            .result
            .as_ref()
            .and_then(|result| result["change_token"].as_str())
            .expect("change_token should be present")
            .to_string();

        let status_no_change = Request::new(
            "agent.status",
            Some(json!({ "workspace_id": workspace_id })),
            RequestId::Number(671),
        );
        let no_change_response = dispatch_request(status_no_change, &state).await;
        assert!(no_change_response.error.is_none(), "expected success: {no_change_response:?}");
        let no_change_token = no_change_response
            .result
            .as_ref()
            .and_then(|result| result["change_token"].as_str())
            .expect("change_token should be present")
            .to_string();
        assert_eq!(before_token, no_change_token, "token should be stable when state is unchanged");

        let edit_request = Request::new(
            "doc.edit",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "client_update_id": "status-token-upd-1",
                "content_md": "# After\n"
            })),
            RequestId::Number(672),
        );
        let edit_response = dispatch_request(edit_request, &state).await;
        assert!(edit_response.error.is_none(), "expected edit to succeed: {edit_response:?}");

        let status_after = Request::new(
            "agent.status",
            Some(json!({ "workspace_id": workspace_id })),
            RequestId::Number(673),
        );
        let after_response = dispatch_request(status_after, &state).await;
        assert!(after_response.error.is_none(), "expected success: {after_response:?}");
        let after_token = after_response
            .result
            .as_ref()
            .and_then(|result| result["change_token"].as_str())
            .expect("change_token should be present")
            .to_string();
        assert_ne!(
            before_token, after_token,
            "token should change when workspace head_seq changes"
        );
    }

    #[tokio::test]
    async fn agent_conflicts_returns_overlapping_section_items() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();

        claim_section(&state, 67, workspace_id, doc_id, "root/auth", "claude-1", "exclusive").await;
        claim_section(&state, 68, workspace_id, doc_id, "root/auth", "copilot-1", "shared").await;
        claim_section(&state, 69, workspace_id, doc_id, "root/api", "cursor-1", "shared").await;

        let request = Request::new(
            "agent.conflicts",
            Some(json!({ "workspace_id": workspace_id, "doc_id": doc_id })),
            RequestId::Number(70),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result should be populated");
        let items = result["items"].as_array().expect("items should be an array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["section"]["id"], "root/auth");
        assert_eq!(items[0]["severity"], "info");
        let editors = items[0]["editors"].as_array().expect("editors should be an array");
        assert_eq!(editors.len(), 2);
        let names = editors
            .iter()
            .map(|editor| editor["name"].as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        assert!(names.contains(&"claude-1"));
        assert!(names.contains(&"copilot-1"));
    }

    #[tokio::test]
    async fn agent_list_aggregates_agents_by_latest_activity() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();

        claim_section(&state, 71, workspace_id, doc_id, "root/auth", "claude-1", "exclusive").await;
        claim_section(&state, 72, workspace_id, doc_id, "root/auth/oauth", "claude-1", "shared")
            .await;
        claim_section(&state, 73, workspace_id, doc_id, "root/api", "copilot-1", "shared").await;

        let request = Request::new(
            "agent.list",
            Some(json!({ "workspace_id": workspace_id })),
            RequestId::Number(74),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result should be populated");
        let items = result["items"].as_array().expect("items should be an array");
        assert_eq!(items.len(), 2);

        let mut sections_by_agent = HashMap::new();
        for item in items {
            sections_by_agent.insert(
                item["agent_id"].as_str().expect("agent_id should be a string").to_string(),
                item["active_sections"].as_u64().expect("active_sections should be numeric") as u32,
            );
            assert!(item["last_seen_at"].as_str().is_some());
        }
        assert_eq!(sections_by_agent.get("claude-1"), Some(&2));
        assert_eq!(sections_by_agent.get("copilot-1"), Some(&1));
    }

    // ── git.status tests ───────────────────────────────────────────────

    #[tokio::test]
    async fn git_status_returns_info_when_configured() {
        let mock = MockGitOps::new().with_status(Ok(GitStatusInfo {
            dirty: true,
            status_output: " M README.md\n".to_string(),
            policy: GitSyncPolicy::AutoRebase,
            ai_configured: true,
            last_sync_at: None,
        }));
        let state = state_with_git(mock);
        let request = Request::new("git.status", None, RequestId::Number(10));
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result should be populated");
        assert_eq!(result["dirty"], true);
        assert_eq!(result["status_output"], " M README.md\n");
        assert_eq!(result["policy"], "auto_rebase");
        assert_eq!(result["ai_configured"], true);
        assert_eq!(result.get("last_sync_at"), None);
    }

    #[tokio::test]
    async fn git_status_errors_when_git_not_configured() {
        let state = RpcServerState::default();
        let request = Request::new("git.status", None, RequestId::Number(11));
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INTERNAL_ERROR);
        assert!(error.message.contains("git not configured"));
    }

    #[tokio::test]
    async fn git_status_returns_error_when_command_fails() {
        let mock = MockGitOps::new().with_status(Err("not a git repository".to_string()));
        let state = state_with_git(mock);
        let request = Request::new("git.status", None, RequestId::Number(12));
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INTERNAL_ERROR);
        assert!(error.message.contains("not a git repository"));
    }

    // ── git.sync tests ─────────────────────────────────────────────────

    #[tokio::test]
    async fn git_sync_commit_returns_job_id() {
        let job_id = Uuid::new_v4();
        let mock = MockGitOps::new().with_sync_result(Ok(job_id));
        let state = state_with_git(mock.clone());
        let request = Request::new(
            "git.sync",
            Some(json!({
                "action": { "commit": { "message": "docs: update" } }
            })),
            RequestId::Number(20),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result should be populated");
        assert_eq!(result["job_id"], json!(job_id));

        let calls = mock.sync_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], "commit:docs: update|trigger:checkpoint");
    }

    #[tokio::test]
    async fn git_sync_commit_and_push_returns_job_id() {
        let job_id = Uuid::new_v4();
        let mock = MockGitOps::new().with_sync_result(Ok(job_id));
        let state = state_with_git(mock.clone());
        let request = Request::new(
            "git.sync",
            Some(json!({
                "action": { "commit_and_push": { "message": "feat: add X" } }
            })),
            RequestId::Number(21),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result should be populated");
        assert_eq!(result["job_id"], json!(job_id));

        let calls = mock.sync_calls.lock().unwrap();
        assert_eq!(calls[0], "commit_and_push:feat: add X|trigger:checkpoint");
    }

    #[tokio::test]
    async fn git_sync_errors_when_git_not_configured() {
        let state = RpcServerState::default();
        let request = Request::new(
            "git.sync",
            Some(json!({ "action": { "commit": { "message": "x" } } })),
            RequestId::Number(22),
        );
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INTERNAL_ERROR);
        assert!(error.message.contains("git not configured"));
    }

    #[tokio::test]
    async fn git_sync_rejects_missing_params() {
        let state = state_with_git(MockGitOps::new());
        let request = Request::new("git.sync", None, RequestId::Number(23));
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INVALID_PARAMS);
    }

    #[tokio::test]
    async fn git_sync_rejects_invalid_params() {
        let state = state_with_git(MockGitOps::new());
        let request =
            Request::new("git.sync", Some(json!({ "action": "bad" })), RequestId::Number(24));
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INVALID_PARAMS);
    }

    #[tokio::test]
    async fn git_sync_returns_error_when_sync_fails() {
        let mock = MockGitOps::new().with_sync_result(Err("nothing to commit".to_string()));
        let state = state_with_git(mock);
        let request = Request::new(
            "git.sync",
            Some(json!({ "action": { "commit": { "message": "x" } } })),
            RequestId::Number(25),
        );
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INTERNAL_ERROR);
        assert!(error.message.contains("nothing to commit"));
    }

    #[tokio::test]
    async fn idle_fallback_trigger_commits_after_inactivity() {
        let mock = MockGitOps::new().with_sync_result(Ok(Uuid::new_v4()));
        let state = state_with_git(mock.clone()).with_git_trigger_config(TriggerConfig {
            min_commit_interval: Duration::from_millis(25),
            idle_fallback_timeout: Duration::from_millis(40),
            max_batch_size: 10,
        });

        state.register_git_change("docs/idle.md");
        tokio::time::sleep(Duration::from_millis(120)).await;

        let calls = mock.sync_calls.lock().expect("sync calls lock should be available");
        assert_eq!(calls.len(), 1);
        assert!(calls[0].starts_with("commit:chore: idle fallback checkpoint"));
        assert!(
            calls[0].contains("|trigger:idle_fallback"),
            "expected trigger metadata in call: {}",
            calls[0]
        );
    }

    #[tokio::test]
    async fn lease_expiry_trigger_commits_after_claim_ttl_expires() {
        let mock = MockGitOps::new().with_sync_result(Ok(Uuid::new_v4()));
        let state = state_with_git(mock.clone()).with_git_trigger_config(TriggerConfig {
            min_commit_interval: Duration::from_millis(25),
            idle_fallback_timeout: Duration::from_secs(5),
            max_batch_size: 10,
        });

        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        state.seed_doc(workspace_id, doc_id, "docs/lease.md", "Lease", "# Lease\n").await;

        let edit_request = Request::new(
            "doc.edit",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "client_update_id": "lease-upd-1",
                "content_md": "# Lease\nupdated\n"
            })),
            RequestId::Number(251),
        );
        let edit_response = dispatch_request(edit_request, &state).await;
        assert!(edit_response.error.is_none(), "edit should succeed: {edit_response:?}");

        let claim_request = Request::new(
            "agent.claim",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
                "section_id": "root/lease",
                "ttl_sec": 1,
                "mode": "exclusive",
                "agent_id": "claude-1"
            })),
            RequestId::Number(252),
        );
        let claim_response = dispatch_request(claim_request, &state).await;
        assert!(claim_response.error.is_none(), "claim should succeed: {claim_response:?}");

        tokio::time::sleep(Duration::from_millis(1250)).await;

        let calls = mock.sync_calls.lock().expect("sync calls lock should be available");
        assert_eq!(calls.len(), 1);
        assert!(
            calls[0].contains("|trigger:lease_released"),
            "expected lease trigger metadata in call: {}",
            calls[0]
        );
    }

    #[tokio::test]
    async fn git_state_sync_uses_ai_generated_message_when_enabled() {
        let executor = MockCommandExecutor::new(vec![
            ok_command(""),
            ok_command("diff --git a/docs/readme.md b/docs/readme.md\n"),
            ok_command("M\tdocs/readme.md\n"),
            ok_command("[main abc123] commit\n"),
        ]);
        let ai_client = Arc::new(MockAiClient::success("feat: ai generated commit"));
        let git = GitState::with_executor_and_ai(
            "/tmp/repo",
            executor.clone(),
            ai_client,
            true,
            AiRedactionPolicy::Full,
        );

        let _ = git
            .sync(GitSyncAction::Commit {
                message: "docs: semantic trigger".to_string(),
                trigger_type: None,
            })
            .await
            .expect("git sync should succeed");

        let calls = executor.calls();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[0].args, vec!["add", "."]);
        assert_eq!(calls[1].args, vec!["diff", "--cached", "--no-color"]);
        assert_eq!(calls[2].args, vec!["diff", "--cached", "--name-status"]);
        assert_eq!(calls[3].args, vec!["commit", "-m", "feat: ai generated commit"]);
    }

    #[tokio::test]
    async fn git_state_sync_falls_back_when_ai_generation_fails() {
        let executor = MockCommandExecutor::new(vec![
            ok_command(""),
            ok_command("diff --git a/docs/readme.md b/docs/readme.md\n"),
            ok_command("M\tdocs/readme.md\n"),
            ok_command("[main abc123] commit\n"),
        ]);
        let ai_client =
            Arc::new(MockAiClient::failure(AiCommitError::ClientError("timeout".into())));
        let git = GitState::with_executor_and_ai(
            "/tmp/repo",
            executor.clone(),
            ai_client,
            true,
            AiRedactionPolicy::Redacted,
        );

        let _ = git
            .sync(GitSyncAction::Commit {
                message: "docs: semantic trigger".to_string(),
                trigger_type: None,
            })
            .await
            .expect("git sync should succeed");

        let calls = executor.calls();
        assert_eq!(calls[3].args, vec!["commit", "-m", "Update 1 file(s): docs/readme.md"]);
    }

    #[tokio::test]
    async fn git_state_sync_appends_trigger_metadata_trailer() {
        let executor = MockCommandExecutor::new(vec![
            ok_command(""),
            ok_command("diff --git a/docs/readme.md b/docs/readme.md\n"),
            ok_command("M\tdocs/readme.md\n"),
            ok_command("[main abc123] commit\n"),
        ]);
        let ai_client =
            Arc::new(MockAiClient::failure(AiCommitError::ClientError("disabled".into())));
        let git = GitState::with_executor_and_ai(
            "/tmp/repo",
            executor.clone(),
            ai_client,
            false,
            AiRedactionPolicy::Disabled,
        );

        let _ = git
            .sync(GitSyncAction::Commit {
                message: "docs: semantic trigger".to_string(),
                trigger_type: Some("checkpoint".to_string()),
            })
            .await
            .expect("git sync should succeed");

        let calls = executor.calls();
        assert_eq!(
            calls[3].args,
            vec![
                "commit",
                "-m",
                "Update 1 file(s): docs/readme.md\n\nScriptum-Trigger: checkpoint",
            ]
        );
    }

    // ── git.configure tests ────────────────────────────────────────────

    #[tokio::test]
    async fn git_configure_sets_policy() {
        let mock = MockGitOps::new();
        let state = state_with_git(mock.clone());
        let request = Request::new(
            "git.configure",
            Some(json!({ "policy": "auto_rebase" })),
            RequestId::Number(30),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result should be populated");
        assert_eq!(result["policy"], "auto_rebase");
        assert_eq!(mock.get_policy(), GitSyncPolicy::AutoRebase);
    }

    #[tokio::test]
    async fn git_configure_errors_when_git_not_configured() {
        let state = RpcServerState::default();
        let request = Request::new(
            "git.configure",
            Some(json!({ "policy": "disabled" })),
            RequestId::Number(31),
        );
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INTERNAL_ERROR);
        assert!(error.message.contains("git not configured"));
    }

    #[tokio::test]
    async fn git_configure_rejects_missing_params() {
        let state = state_with_git(MockGitOps::new());
        let request = Request::new("git.configure", None, RequestId::Number(32));
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INVALID_PARAMS);
    }

    #[tokio::test]
    async fn git_configure_rejects_invalid_policy() {
        let state = state_with_git(MockGitOps::new());
        let request = Request::new(
            "git.configure",
            Some(json!({ "policy": "turbo_mode" })),
            RequestId::Number(33),
        );
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INVALID_PARAMS);
    }

    // ── git.status clean working tree ──────────────────────────────────

    #[tokio::test]
    async fn git_status_clean_working_tree_is_not_dirty() {
        let mock = MockGitOps::new().with_status(Ok(GitStatusInfo {
            dirty: false,
            status_output: String::new(),
            policy: GitSyncPolicy::Disabled,
            ai_configured: false,
            last_sync_at: None,
        }));
        let state = state_with_git(mock);
        let request = Request::new("git.status", None, RequestId::Number(40));
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result should be populated");
        assert_eq!(result["dirty"], false);
        assert_eq!(result["policy"], "disabled");
        assert_eq!(result["ai_configured"], false);
    }

    // ── git.configure round-trip ────────────────────────────────────

    #[tokio::test]
    async fn git_configure_round_trips_all_policies() {
        let mock = MockGitOps::new();
        let state = state_with_git(mock.clone());

        for (policy_str, expected) in [
            ("disabled", GitSyncPolicy::Disabled),
            ("manual", GitSyncPolicy::Manual),
            ("auto_rebase", GitSyncPolicy::AutoRebase),
        ] {
            let request = Request::new(
                "git.configure",
                Some(json!({ "policy": policy_str })),
                RequestId::Number(50),
            );
            let response = dispatch_request(request, &state).await;
            assert!(response.error.is_none(), "policy {policy_str}: {response:?}");
            assert_eq!(mock.get_policy(), expected);
        }
    }

    // ── doc.sections tests ────────────────────────────────────────

    #[tokio::test]
    async fn doc_sections_returns_section_list() {
        let state = RpcServerState::default();
        let ws = Uuid::new_v4();
        let doc = Uuid::new_v4();
        state.seed_doc(ws, doc, "guide.md", "Guide", "# Guide\n\n## Setup\n\n## Usage\n").await;

        let request = Request::new(
            "doc.sections",
            Some(json!({ "workspace_id": ws, "doc_id": doc })),
            RequestId::Number(200),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result");
        assert_eq!(result["doc_id"], json!(doc));
        let sections = result["sections"].as_array().unwrap();
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0]["heading"], "Guide");
        assert_eq!(sections[1]["heading"], "Setup");
        assert_eq!(sections[2]["heading"], "Usage");
    }

    #[tokio::test]
    async fn doc_sections_returns_empty_for_no_headings() {
        let state = RpcServerState::default();
        let ws = Uuid::new_v4();
        let doc = Uuid::new_v4();
        state.seed_doc(ws, doc, "plain.md", "Plain", "Just some text without headings.\n").await;

        let request = Request::new(
            "doc.sections",
            Some(json!({ "workspace_id": ws, "doc_id": doc })),
            RequestId::Number(201),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result");
        let sections = result["sections"].as_array().unwrap();
        assert!(sections.is_empty());
    }

    #[tokio::test]
    async fn doc_sections_rejects_missing_params() {
        let state = RpcServerState::default();
        let request = Request::new("doc.sections", None, RequestId::Number(202));
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INVALID_PARAMS);
    }

    // ── doc.tree tests ───────────────────────────────────────────────

    #[tokio::test]
    async fn doc_tree_returns_all_docs_in_workspace() {
        let state = RpcServerState::default();
        let ws = Uuid::new_v4();
        state.seed_doc(ws, Uuid::new_v4(), "docs/api.md", "API", "# API\n").await;
        state.seed_doc(ws, Uuid::new_v4(), "docs/guide.md", "Guide", "# Guide\n").await;
        state.seed_doc(ws, Uuid::new_v4(), "readme.md", "README", "# README\n").await;

        let request =
            Request::new("doc.tree", Some(json!({ "workspace_id": ws })), RequestId::Number(210));
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result");
        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(result["total"], 3);
        // Sorted by path.
        assert_eq!(items[0]["path"], "docs/api.md");
        assert_eq!(items[1]["path"], "docs/guide.md");
        assert_eq!(items[2]["path"], "readme.md");
    }

    #[tokio::test]
    async fn doc_tree_filters_by_path_prefix() {
        let state = RpcServerState::default();
        let ws = Uuid::new_v4();
        state.seed_doc(ws, Uuid::new_v4(), "docs/api.md", "API", "# API\n").await;
        state.seed_doc(ws, Uuid::new_v4(), "docs/guide.md", "Guide", "# Guide\n").await;
        state.seed_doc(ws, Uuid::new_v4(), "readme.md", "README", "# README\n").await;

        let request = Request::new(
            "doc.tree",
            Some(json!({ "workspace_id": ws, "path_prefix": "docs/" })),
            RequestId::Number(211),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result");
        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(result["total"], 2);
        assert_eq!(items[0]["path"], "docs/api.md");
        assert_eq!(items[1]["path"], "docs/guide.md");
    }

    #[tokio::test]
    async fn doc_tree_returns_empty_for_unknown_workspace() {
        let state = RpcServerState::default();
        let request = Request::new(
            "doc.tree",
            Some(json!({ "workspace_id": Uuid::new_v4() })),
            RequestId::Number(212),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result");
        assert_eq!(result["total"], 0);
        assert_eq!(result["items"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn doc_tree_excludes_other_workspaces() {
        let state = RpcServerState::default();
        let ws_a = Uuid::new_v4();
        let ws_b = Uuid::new_v4();
        state.seed_doc(ws_a, Uuid::new_v4(), "a.md", "A", "# A\n").await;
        state.seed_doc(ws_b, Uuid::new_v4(), "b.md", "B", "# B\n").await;

        let request =
            Request::new("doc.tree", Some(json!({ "workspace_id": ws_a })), RequestId::Number(213));
        let response = dispatch_request(request, &state).await;

        let result = response.result.expect("result");
        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["path"], "a.md");
    }

    #[tokio::test]
    async fn doc_tree_rejects_missing_params() {
        let state = RpcServerState::default();
        let request = Request::new("doc.tree", None, RequestId::Number(214));
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INVALID_PARAMS);
    }

    // ── doc.search tests ─────────────────────────────────────────────

    #[tokio::test]
    async fn doc_search_returns_paged_workspace_results() {
        let state = RpcServerState::default();
        let ws = Uuid::new_v4();
        state
            .seed_doc(
                ws,
                Uuid::new_v4(),
                "docs/alpha.md",
                "Alpha",
                "# Alpha\n\nScriptum search supports markdown docs.\n",
            )
            .await;
        state
            .seed_doc(
                ws,
                Uuid::new_v4(),
                "docs/beta.md",
                "Beta",
                "# Beta\n\nAnother Scriptum search result.\n",
            )
            .await;
        state
            .seed_doc(
                ws,
                Uuid::new_v4(),
                "docs/gamma.md",
                "Gamma",
                "# Gamma\n\nNo matching term here.\n",
            )
            .await;

        let first_request = Request::new(
            "doc.search",
            Some(json!({ "workspace_id": ws, "q": "Scriptum", "limit": 1 })),
            RequestId::Number(215),
        );
        let first_response = dispatch_request(first_request, &state).await;
        assert!(first_response.error.is_none(), "expected success: {first_response:?}");
        let first_result = first_response.result.expect("result");
        let first_items = first_result["items"].as_array().expect("items should be an array");
        assert_eq!(first_items.len(), 1);
        assert_eq!(first_result["total"], 2);
        let first_path =
            first_items[0]["path"].as_str().expect("path should be a string").to_string();
        assert!(first_path == "docs/alpha.md" || first_path == "docs/beta.md");
        let first_title = first_items[0]["title"].as_str().expect("title should be a string");
        if first_path == "docs/alpha.md" {
            assert_eq!(first_title, "Alpha");
        } else {
            assert_eq!(first_title, "Beta");
        }
        assert!(first_items[0]["doc_id"].as_str().is_some());
        assert!(first_items[0]["snippet"].as_str().is_some());
        assert!(first_items[0]["score"].as_f64().is_some());
        let cursor = first_result["next_cursor"]
            .as_str()
            .expect("next_cursor should be present")
            .to_string();

        let second_request = Request::new(
            "doc.search",
            Some(json!({ "workspace_id": ws, "q": "Scriptum", "limit": 1, "cursor": cursor })),
            RequestId::Number(216),
        );
        let second_response = dispatch_request(second_request, &state).await;
        assert!(second_response.error.is_none(), "expected success: {second_response:?}");
        let second_result = second_response.result.expect("result");
        let second_items = second_result["items"].as_array().expect("items should be an array");
        assert_eq!(second_items.len(), 1);
        assert_eq!(second_result["total"], 2);
        let second_path = second_items[0]["path"].as_str().expect("path should be a string");
        assert!(second_path == "docs/alpha.md" || second_path == "docs/beta.md");
        assert_ne!(second_path, first_path);
        assert_eq!(second_result.get("next_cursor"), None);
    }

    #[tokio::test]
    async fn doc_search_excludes_other_workspaces() {
        let state = RpcServerState::default();
        let ws_a = Uuid::new_v4();
        let ws_b = Uuid::new_v4();
        state
            .seed_doc(
                ws_a,
                Uuid::new_v4(),
                "docs/a.md",
                "A",
                "# A\n\nScriptum token appears here.\n",
            )
            .await;
        state
            .seed_doc(
                ws_b,
                Uuid::new_v4(),
                "docs/b.md",
                "B",
                "# B\n\nScriptum token appears in another workspace.\n",
            )
            .await;

        let request = Request::new(
            "doc.search",
            Some(json!({ "workspace_id": ws_a, "q": "Scriptum", "limit": 10 })),
            RequestId::Number(217),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result");
        let items = result["items"].as_array().expect("items should be an array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["path"], "docs/a.md");
        assert_eq!(result["total"], 1);
    }

    #[tokio::test]
    async fn doc_search_reflects_doc_edit_index_updates() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        state
            .seed_doc(
                workspace_id,
                doc_id,
                "docs/live-index.md",
                "Live Index",
                "# Live Index\n\nLegacy token text.\n",
            )
            .await;

        let edit_response = dispatch_request(
            Request::new(
                "doc.edit",
                Some(json!({
                    "workspace_id": workspace_id,
                    "doc_id": doc_id,
                    "client_update_id": "search-reindex-1",
                    "content_md": "# Live Index\n\nFresh token appears now.\n",
                })),
                RequestId::Number(218),
            ),
            &state,
        )
        .await;
        assert!(edit_response.error.is_none(), "doc.edit should succeed: {edit_response:?}");

        let fresh_search = dispatch_request(
            Request::new(
                "doc.search",
                Some(json!({ "workspace_id": workspace_id, "q": "Fresh", "limit": 10 })),
                RequestId::Number(219),
            ),
            &state,
        )
        .await;
        assert!(fresh_search.error.is_none(), "doc.search should succeed: {fresh_search:?}");
        let fresh_result = fresh_search.result.expect("search result should be present");
        assert_eq!(fresh_result["total"], 1);
        let fresh_items = fresh_result["items"].as_array().expect("items should be an array");
        assert_eq!(fresh_items.len(), 1);
        assert_eq!(fresh_items[0]["doc_id"], doc_id.to_string());

        let stale_search = dispatch_request(
            Request::new(
                "doc.search",
                Some(json!({
                    "workspace_id": workspace_id,
                    "q": "Legacy",
                    "limit": 10
                })),
                RequestId::Number(220),
            ),
            &state,
        )
        .await;
        assert!(stale_search.error.is_none(), "doc.search should succeed: {stale_search:?}");
        let stale_result = stale_search.result.expect("search result should be present");
        assert_eq!(stale_result["total"], 0);
    }

    #[tokio::test]
    async fn doc_search_rejects_invalid_cursor() {
        let state = RpcServerState::default();
        let request = Request::new(
            "doc.search",
            Some(json!({
                "workspace_id": Uuid::new_v4(),
                "q": "Scriptum",
                "cursor": "not-base64"
            })),
            RequestId::Number(221),
        );
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INVALID_PARAMS);
    }

    // ── workspace.list tests ────────────────────────────────────────

    #[tokio::test]
    async fn workspace_list_returns_empty_initially() {
        let state = RpcServerState::default();
        let request = Request::new("workspace.list", None, RequestId::Number(100));
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result");
        assert_eq!(result["items"].as_array().unwrap().len(), 0);
        assert_eq!(result["next_cursor"], serde_json::Value::Null);
        assert_eq!(result["total"], 0);
    }

    #[tokio::test]
    async fn workspace_list_returns_seeded_workspaces() {
        let state = RpcServerState::default();
        let ws_a = Uuid::new_v4();
        let ws_b = Uuid::new_v4();
        state.seed_workspace(ws_a, "Alpha", "/tmp/alpha").await;
        state.seed_workspace(ws_b, "Beta", "/tmp/beta").await;

        // Seed a doc in workspace A so doc_count is reflected.
        state.seed_doc(ws_a, Uuid::new_v4(), "notes.md", "Notes", "# Notes\n").await;

        let request = Request::new("workspace.list", None, RequestId::Number(101));
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result");
        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(result["next_cursor"], serde_json::Value::Null);
        assert_eq!(result["total"], 2);

        // Sorted by name: Alpha before Beta.
        assert_eq!(items[0]["name"], "Alpha");
        assert_eq!(items[0]["workspace_id"], json!(ws_a));
        assert_eq!(items[0]["id"], json!(ws_a));
        assert_eq!(items[0]["slug"], "alpha");
        assert_eq!(items[0]["doc_count"], 1);
        assert_eq!(items[1]["name"], "Beta");
        assert_eq!(items[1]["workspace_id"], json!(ws_b));
        assert_eq!(items[1]["id"], json!(ws_b));
        assert_eq!(items[1]["slug"], "beta");
        assert_eq!(items[1]["doc_count"], 0);
    }

    #[tokio::test]
    async fn workspace_list_respects_pagination() {
        let state = RpcServerState::default();
        for i in 0..5 {
            state
                .seed_workspace(Uuid::new_v4(), format!("WS-{i:02}"), format!("/tmp/ws-{i}"))
                .await;
        }

        let request = Request::new(
            "workspace.list",
            Some(json!({ "offset": 2, "limit": 2 })),
            RequestId::Number(102),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result");
        let items = result["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(result["next_cursor"], "4");
        assert_eq!(result["total"], 5);
    }

    // ── workspace.open tests ────────────────────────────────────────

    #[tokio::test]
    async fn workspace_open_returns_workspace_info() {
        let state = RpcServerState::default();
        let ws_id = Uuid::new_v4();
        state.seed_workspace(ws_id, "MyProject", "/projects/my-project").await;
        state.seed_doc(ws_id, Uuid::new_v4(), "readme.md", "README", "# README\n").await;
        state.seed_doc(ws_id, Uuid::new_v4(), "notes.md", "Notes", "# Notes\n").await;

        let request = Request::new(
            "workspace.open",
            Some(json!({ "workspace_id": ws_id })),
            RequestId::Number(110),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result");
        assert_eq!(result["workspace"]["id"], json!(ws_id));
        assert_eq!(result["workspace"]["name"], "MyProject");
        assert_eq!(result["workspace_id"], json!(ws_id));
        assert_eq!(result["name"], "MyProject");
        assert_eq!(result["root_path"], "/projects/my-project");
        assert_eq!(result["doc_count"], 2);
    }

    #[tokio::test]
    async fn workspace_open_rejects_unknown_id() {
        let state = RpcServerState::default();
        let request = Request::new(
            "workspace.open",
            Some(json!({ "workspace_id": Uuid::new_v4() })),
            RequestId::Number(111),
        );
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INTERNAL_ERROR);
        assert!(error.message.contains("not found"));
    }

    #[tokio::test]
    async fn workspace_open_registers_workspace_from_root_path() {
        let global_config_root = tempfile::tempdir().expect("tempdir should be created");
        let global_config_path = global_config_root.path().join("config.toml");
        let workspace_root = tempfile::tempdir().expect("workspace root tempdir should be created");
        let workspace_id = Uuid::new_v4();

        let mut workspace_config = crate::config::WorkspaceConfig::default();
        workspace_config.sync.workspace_id = Some(workspace_id.to_string());
        workspace_config.sync.workspace_name = Some("Opened Workspace".to_string());
        workspace_config.save(workspace_root.path()).expect("workspace config should be saved");

        let state = RpcServerState::default().with_global_config_path(global_config_path.clone());
        let request = Request::new(
            "workspace.open",
            Some(json!({
                "root_path": workspace_root.path().to_str().expect("workspace path should be UTF-8")
            })),
            RequestId::Number(113),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result should be present");
        assert_eq!(result["workspace_id"], json!(workspace_id));
        assert_eq!(result["name"], "Opened Workspace");
        assert_eq!(result["doc_count"], 0);

        let listed =
            dispatch_request(Request::new("workspace.list", None, RequestId::Number(114)), &state)
                .await;
        assert!(listed.error.is_none(), "workspace.list should succeed: {listed:?}");
        let listed_result = listed.result.expect("workspace.list result should be present");
        assert_eq!(listed_result["total"], 1);
        assert_eq!(listed_result["items"][0]["workspace_id"], json!(workspace_id));

        let persisted = crate::config::GlobalConfig::load_from(&global_config_path)
            .expect("global config should be persisted");
        let canonical_root = workspace_root
            .path()
            .canonicalize()
            .expect("workspace path should canonicalize")
            .to_string_lossy()
            .to_string();
        assert_eq!(persisted.workspace_paths, vec![canonical_root]);
    }

    #[tokio::test]
    async fn workspace_open_rejects_ambiguous_params() {
        let state = RpcServerState::default();
        let request = Request::new(
            "workspace.open",
            Some(json!({
                "workspace_id": Uuid::new_v4(),
                "root_path": "/tmp/project"
            })),
            RequestId::Number(115),
        );
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INVALID_PARAMS);
        assert!(error.data.expect("error data should be present")["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("either workspace_id or root_path"));
    }

    #[tokio::test]
    async fn workspace_open_rejects_missing_params() {
        let state = RpcServerState::default();
        let request = Request::new("workspace.open", None, RequestId::Number(112));
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INVALID_PARAMS);
    }

    // ── workspace.create tests ──────────────────────────────────────

    #[tokio::test]
    async fn workspace_create_initializes_directory_and_registers() {
        let state = RpcServerState::default();
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_str().unwrap().to_string();

        let request = Request::new(
            "workspace.create",
            Some(json!({ "name": "New Project", "root_path": root })),
            RequestId::Number(120),
        );
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result");
        let canonical_root = tmp
            .path()
            .canonicalize()
            .expect("workspace root should canonicalize")
            .to_string_lossy()
            .to_string();
        assert_eq!(result["workspace"]["name"], "New Project");
        assert_eq!(result["workspace"]["slug"], "new-project");
        assert_eq!(result["name"], "New Project");
        assert_eq!(result["root_path"], json!(canonical_root));
        let workspace_id =
            result["workspace_id"].as_str().expect("workspace_id should be present").to_string();
        assert!(result["workspace"]["id"].as_str().is_some());
        assert!(result["created_at"].as_str().is_some());

        // Verify expected workspace-local storage layout.
        let scriptum_dir = tmp.path().join(".scriptum");
        let toml_path = scriptum_dir.join("workspace.toml");
        let wal_dir = scriptum_dir.join("crdt_store").join("wal");
        let snapshots_dir = scriptum_dir.join("crdt_store").join("snapshots");
        let meta_db_path = scriptum_dir.join("meta.db");

        assert!(toml_path.exists(), ".scriptum/workspace.toml should exist");
        assert!(wal_dir.is_dir(), ".scriptum/crdt_store/wal should exist");
        assert!(snapshots_dir.is_dir(), ".scriptum/crdt_store/snapshots should exist");
        assert!(meta_db_path.is_file(), ".scriptum/meta.db should exist");

        // Verify workspace identity persisted to workspace.toml.
        let workspace_config = crate::config::WorkspaceConfig::load(tmp.path());
        assert_eq!(workspace_config.sync.workspace_id.as_deref(), Some(workspace_id.as_str()));
        assert_eq!(workspace_config.sync.workspace_name.as_deref(), Some("New Project"));

        // Verify workspace meta.db was initialized with schema.
        let workspace_meta_db =
            crate::store::meta_db::MetaDb::open(&meta_db_path).expect("meta.db should open");
        let documents_table_exists: i64 = workspace_meta_db
            .connection()
            .query_row(
                "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = 'documents_local'",
                [],
                |row| row.get(0),
            )
            .expect("documents_local table query should succeed");
        assert_eq!(documents_table_exists, 1, "documents_local table should exist");

        // Verify workspace is now listed.
        let list_req = Request::new("workspace.list", None, RequestId::Number(121));
        let list_resp = dispatch_request(list_req, &state).await;
        let list_result = list_resp.result.expect("result");
        assert_eq!(list_result["total"], 1);
        assert_eq!(list_result["items"][0]["name"], "New Project");
    }

    #[tokio::test]
    async fn workspace_create_recovered_after_restart() {
        let daemon_home = tempfile::tempdir().expect("tempdir should be created");
        let global_config_path = daemon_home.path().join("config.toml");
        let workspace_root = tempfile::tempdir().expect("workspace root should be created");

        let state_before_restart =
            RpcServerState::default().with_global_config_path(global_config_path.clone());
        let create = Request::new(
            "workspace.create",
            Some(json!({
                "name": "Restartable Workspace",
                "root_path": workspace_root.path().to_str().expect("workspace path should be UTF-8"),
            })),
            RequestId::Number(126),
        );
        let create_resp = dispatch_request(create, &state_before_restart).await;
        assert!(create_resp.error.is_none(), "workspace.create should succeed: {create_resp:?}");
        let create_result = create_resp.result.expect("workspace.create result should be present");
        let workspace_id = create_result["workspace_id"].clone();

        let state_after_restart =
            RpcServerState::default().with_global_config_path(global_config_path.clone());
        let report = state_after_restart.recover_workspaces_at_startup().await;
        assert_eq!(report.registered_workspaces, 1);
        assert_eq!(report.skipped_paths, 0);

        let list_resp = dispatch_request(
            Request::new("workspace.list", None, RequestId::Number(127)),
            &state_after_restart,
        )
        .await;
        assert!(list_resp.error.is_none(), "workspace.list should succeed: {list_resp:?}");
        let list_result = list_resp.result.expect("workspace.list result should be present");
        assert_eq!(list_result["total"], 1);
        assert_eq!(list_result["items"][0]["workspace_id"], workspace_id);
    }

    #[tokio::test]
    async fn startup_workspace_recovery_skips_missing_or_corrupt_paths() {
        let daemon_home = tempfile::tempdir().expect("tempdir should be created");
        let global_config_path = daemon_home.path().join("config.toml");
        let missing_workspace = daemon_home.path().join("missing-workspace");

        let corrupt_workspace = tempfile::tempdir().expect("corrupt workspace should be created");
        let corrupt_config_dir = corrupt_workspace.path().join(".scriptum");
        std::fs::create_dir_all(&corrupt_config_dir)
            .expect("corrupt workspace .scriptum should be created");
        std::fs::write(corrupt_config_dir.join("workspace.toml"), "not = [valid")
            .expect("corrupt workspace.toml should be written");

        let mut global_config = crate::config::GlobalConfig::default();
        global_config.workspace_paths = vec![
            missing_workspace.to_string_lossy().to_string(),
            corrupt_workspace
                .path()
                .canonicalize()
                .expect("corrupt workspace path should canonicalize")
                .to_string_lossy()
                .to_string(),
        ];
        global_config.save_to(&global_config_path).expect("global config should be written");

        let state = RpcServerState::default().with_global_config_path(global_config_path);
        let report = state.recover_workspaces_at_startup().await;
        assert_eq!(report.registered_workspaces, 0);
        assert_eq!(report.skipped_paths, 2);
    }

    #[tokio::test]
    async fn workspace_create_rejects_empty_name() {
        let state = RpcServerState::default();
        let request = Request::new(
            "workspace.create",
            Some(json!({ "name": "  ", "root_path": "/tmp/x" })),
            RequestId::Number(122),
        );
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INVALID_PARAMS);
        assert!(error.data.unwrap()["reason"].as_str().unwrap().contains("must not be empty"));
    }

    #[tokio::test]
    async fn workspace_create_rejects_relative_path() {
        let state = RpcServerState::default();
        let request = Request::new(
            "workspace.create",
            Some(json!({ "name": "Bad", "root_path": "relative/path" })),
            RequestId::Number(123),
        );
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INVALID_PARAMS);
        assert!(error.data.unwrap()["reason"].as_str().unwrap().contains("absolute path"));
    }

    #[tokio::test]
    async fn workspace_create_rejects_missing_params() {
        let state = RpcServerState::default();
        let request = Request::new("workspace.create", None, RequestId::Number(124));
        let response = dispatch_request(request, &state).await;

        let error = response.error.expect("error should be present");
        assert_eq!(error.code, INVALID_PARAMS);
    }

    #[tokio::test]
    async fn workspace_create_imports_existing_markdown_files_and_indexes() {
        let state = RpcServerState::default();
        let tmp = tempfile::tempdir().expect("tempdir should be created");

        std::fs::create_dir_all(tmp.path().join("docs")).expect("docs directory should be created");
        std::fs::write(
            tmp.path().join("docs").join("guide.md"),
            "# Guide\n\nSee [[notes]].\n#ops #Migration\n",
        )
        .expect("guide markdown should be written");
        std::fs::write(
            tmp.path().join("notes.md"),
            b"\xEF\xBB\xBF# Notes\r\n\r\nWindows line endings.\r\n",
        )
        .expect("notes markdown should be written");
        std::fs::write(tmp.path().join("ignore.txt"), "not markdown")
            .expect("non-markdown file should be written");

        let create = Request::new(
            "workspace.create",
            Some(json!({
                "name": "Imported Workspace",
                "root_path": tmp.path().to_str().expect("workspace root should be UTF-8"),
            })),
            RequestId::Number(125),
        );
        let create_resp = dispatch_request(create, &state).await;
        assert!(create_resp.error.is_none(), "workspace.create should succeed: {create_resp:?}");
        let create_result = create_resp.result.expect("workspace.create result should be present");
        let workspace_id: Uuid = serde_json::from_value(create_result["workspace"]["id"].clone())
            .expect("workspace_id should decode");

        let tree_resp = dispatch_request(
            Request::new(
                "doc.tree",
                Some(json!({ "workspace_id": workspace_id })),
                RequestId::Number(126),
            ),
            &state,
        )
        .await;
        assert!(tree_resp.error.is_none(), "doc.tree should succeed");
        let tree = tree_resp.result.expect("doc.tree result should be present");
        assert_eq!(tree["total"], 2);

        let mut docs_by_path = HashMap::new();
        for item in tree["items"].as_array().expect("doc.tree items should be an array") {
            let path = item["path"].as_str().expect("doc.tree path should be a string");
            let doc_id = item["doc_id"].as_str().expect("doc.tree doc_id should be a string");
            docs_by_path
                .insert(path.to_string(), Uuid::parse_str(doc_id).expect("doc_id should parse"));
        }
        assert!(docs_by_path.contains_key("docs/guide.md"));
        assert!(docs_by_path.contains_key("notes.md"));

        let notes_doc_id = docs_by_path["notes.md"];
        let read_resp = dispatch_request(
            Request::new(
                "doc.read",
                Some(json!({
                    "workspace_id": workspace_id,
                    "doc_id": notes_doc_id,
                    "include_content": true,
                })),
                RequestId::Number(127),
            ),
            &state,
        )
        .await;
        assert!(read_resp.error.is_none(), "doc.read should succeed");
        let read_result = read_resp.result.expect("doc.read result should be present");
        let notes_content =
            read_result["content_md"].as_str().expect("doc.read should include markdown content");
        assert!(
            !notes_content.contains('\u{feff}'),
            "BOM should be stripped from imported content"
        );
        assert!(notes_content.contains("\r\n"), "CRLF line endings should be preserved");

        let guide_doc_id = docs_by_path["docs/guide.md"];
        let (source_doc_id, target_doc_id, indexed_tags) = state
            .with_agent_storage(|conn, _| {
                let source_doc_id: String = conn
                    .query_row("SELECT source_doc_id FROM backlinks LIMIT 1", [], |row| row.get(0))
                    .map_err(|error| format!("failed to query backlink source: {error}"))?;
                let target_doc_id: String = conn
                    .query_row("SELECT target_doc_id FROM backlinks LIMIT 1", [], |row| row.get(0))
                    .map_err(|error| format!("failed to query backlink target: {error}"))?;
                let indexed_tags: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM document_tags WHERE doc_id = ?1",
                        rusqlite::params![guide_doc_id.to_string()],
                        |row| row.get(0),
                    )
                    .map_err(|error| format!("failed to count indexed tags: {error}"))?;
                Ok((source_doc_id, target_doc_id, indexed_tags))
            })
            .expect("index rows should be queryable");

        assert_eq!(source_doc_id, guide_doc_id.to_string());
        assert_eq!(target_doc_id, notes_doc_id.to_string());
        assert_eq!(indexed_tags, 2);
    }

    #[test]
    fn workspace_import_detects_path_norm_collisions() {
        let err = super::ensure_unique_path_norm(&[
            "docs/file.md".to_string(),
            "docs\\file.md".to_string(),
        ])
        .expect_err("equivalent normalized paths should be rejected");
        assert!(err.contains("path collision after normalization"));
    }
}
