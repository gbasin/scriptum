use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use crate::agent::lease::{LeaseClaim, LeaseMode, LeaseStore};
use crate::agent::session::{AgentSession as PersistedAgentSession, SessionStatus, SessionStore};
use crate::engine::{doc_manager::DocManager, ydoc::YDoc};
use crate::git::worker::{CommandExecutor, GitWorker, ProcessCommandExecutor};
use crate::search::{Fts5Index, IndexEntry, SearchIndex};
use crate::store::meta_db::MetaDb;
use crate::store::recovery::{recover_documents_into_manager, StartupRecoveryReport};
use base64::Engine;
use scriptum_common::protocol::jsonrpc::{
    Request, RequestId, Response, RpcError, INTERNAL_ERROR, INVALID_PARAMS, INVALID_REQUEST,
    METHOD_NOT_FOUND, PARSE_ERROR,
};
use scriptum_common::section::parser::parse_sections;
use scriptum_common::types::{
    AgentSession as RpcAgentSession, EditorType, OverlapEditor, OverlapSeverity, Section,
    SectionOverlap,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast;
use tokio::sync::RwLock;
use tiktoken_rs::CoreBPE;
use uuid::Uuid;

// ── Git sync policy ─────────────────────────────────────────────────

/// Controls git sync behavior for this workspace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GitSyncPolicy {
    /// No automatic git operations.
    Disabled,
    /// Commit on triggers but never push.
    Manual,
    /// Commit + push with rebase on triggers.
    AutoRebase,
}

impl Default for GitSyncPolicy {
    fn default() -> Self {
        Self::Manual
    }
}

// ── Git state ───────────────────────────────────────────────────────

/// Git-related state for the RPC server.
#[derive(Clone)]
pub struct GitState<E: CommandExecutor = ProcessCommandExecutor> {
    worker: Arc<GitWorker<E>>,
    policy: Arc<RwLock<GitSyncPolicy>>,
    last_sync_at: Arc<RwLock<Option<chrono::DateTime<chrono::Utc>>>>,
}

impl GitState<ProcessCommandExecutor> {
    pub fn new(repo_path: impl Into<PathBuf>) -> Self {
        Self {
            worker: Arc::new(GitWorker::new(repo_path)),
            policy: Arc::new(RwLock::new(GitSyncPolicy::default())),
            last_sync_at: Arc::new(RwLock::new(None)),
        }
    }
}

impl<E: CommandExecutor> GitState<E> {
    pub fn with_executor(repo_path: impl Into<PathBuf>, executor: E) -> Self {
        Self {
            worker: Arc::new(GitWorker::with_executor(repo_path, executor)),
            policy: Arc::new(RwLock::new(GitSyncPolicy::default())),
            last_sync_at: Arc::new(RwLock::new(None)),
        }
    }
}

#[derive(Clone)]
pub struct RpcServerState {
    doc_manager: Arc<RwLock<DocManager>>,
    doc_metadata: Arc<RwLock<HashMap<(Uuid, Uuid), DocMetadataRecord>>>,
    doc_history: Arc<RwLock<HashMap<(Uuid, Uuid), BTreeMap<i64, String>>>>,
    degraded_docs: Arc<RwLock<HashSet<Uuid>>>,
    workspaces: Arc<RwLock<HashMap<Uuid, WorkspaceInfo>>>,
    shutdown_notifier: Option<broadcast::Sender<()>>,
    git_state: Option<Arc<dyn GitOps + Send + Sync>>,
    agent_db: Arc<Mutex<MetaDb>>,
    lease_store: Arc<Mutex<LeaseStore>>,
    agent_id: Arc<String>,
}

/// Trait to abstract git operations for testability via dynamic dispatch.
trait GitOps: Send + Sync {
    fn status_info(&self) -> Result<GitStatusInfo, String>;
    fn sync(&self, action: GitSyncAction) -> Result<Uuid, String>;
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
        })
    }

    fn sync(&self, action: GitSyncAction) -> Result<Uuid, String> {
        let job_id = Uuid::new_v4();

        match action {
            GitSyncAction::Commit { message } => {
                // Stage all tracked changes.
                let _ = self.worker.add(&["."]).map_err(|e| e.to_string())?;
                self.worker.commit(&message).map_err(|e| e.to_string())?;
            }
            GitSyncAction::CommitAndPush { message } => {
                let _ = self.worker.add(&["."]).map_err(|e| e.to_string())?;
                self.worker.commit(&message).map_err(|e| e.to_string())?;
                self.worker.push().map_err(|e| e.to_string())?;
            }
        }

        self.mark_synced();
        Ok(job_id)
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
}

#[derive(Debug, Clone, Serialize)]
struct DocReadResult {
    metadata: DocMetadataRecord,
    sections: Vec<Section>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_md: Option<String>,
    degraded: bool,
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
    from_seq: i64,
    to_seq: i64,
}

#[derive(Debug, Clone, Serialize)]
struct DocDiffResult {
    patch_md: String,
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
    thread_id: String,
    section_id: String,
    excerpt: String,
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

#[derive(Debug, Clone, Serialize)]
struct WorkspaceListItem {
    workspace_id: Uuid,
    name: String,
    root_path: String,
    doc_count: usize,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize)]
struct WorkspaceListResult {
    items: Vec<WorkspaceListItem>,
    total: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkspaceOpenParams {
    workspace_id: Uuid,
}

#[derive(Debug, Clone, Serialize)]
struct WorkspaceOpenResult {
    workspace_id: Uuid,
    name: String,
    root_path: String,
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
    workspace_id: Uuid,
    name: String,
    root_path: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl Default for RpcServerState {
    fn default() -> Self {
        let meta_db = MetaDb::open(":memory:").expect("in-memory meta.db should initialize");
        let lease_store = LeaseStore::new(meta_db.connection(), chrono::Utc::now())
            .expect("lease store should initialize");
        Self {
            doc_manager: Arc::new(RwLock::new(DocManager::default())),
            doc_metadata: Arc::new(RwLock::new(HashMap::new())),
            doc_history: Arc::new(RwLock::new(HashMap::new())),
            degraded_docs: Arc::new(RwLock::new(HashSet::new())),
            workspaces: Arc::new(RwLock::new(HashMap::new())),
            shutdown_notifier: None,
            git_state: None,
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

    pub fn with_agent_identity(mut self, agent_id: impl Into<String>) -> Self {
        self.agent_id = Arc::new(agent_id.into());
        self
    }

    fn with_agent_storage<T, F>(&self, f: F) -> Result<T, String>
    where
        F: FnOnce(&rusqlite::Connection, &mut LeaseStore) -> Result<T, String>,
    {
        let db = self
            .agent_db
            .lock()
            .map_err(|_| "agent db lock poisoned".to_string())?;
        let mut leases = self
            .lease_store
            .lock()
            .map_err(|_| "agent lease store lock poisoned".to_string())?;
        f(db.connection(), &mut leases)
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

        if let Some(session) = active_sessions.into_iter().find(|session| session.agent_id == agent_id)
        {
            SessionStore::touch(conn, &session.session_id, now).map_err(|error| error.to_string())?;
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

    fn agent_status(&self, workspace_id: Uuid) -> Result<AgentStatusResult, String> {
        let now = chrono::Utc::now();
        self.with_agent_storage(|conn, lease_store| {
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
            Ok(AgentStatusResult { active_sessions })
        })
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
                b.last_seen_at
                    .cmp(&a.last_seen_at)
                    .then_with(|| a.agent_id.cmp(&b.agent_id))
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

        self.with_agent_storage(|conn, lease_store| {
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

            let claim_result = lease_store.claim(conn, claim, now).map_err(|error| error.to_string())?;
            let lease = claim_result.lease;
            let conflicts = claim_result
                .conflicts
                .into_iter()
                .map(|conflict| AgentClaimConflictResult {
                    agent_id: conflict.agent_id,
                    section_id: conflict.section_id,
                })
                .collect::<Vec<_>>();

            Ok(AgentClaimResult {
                lease_id: format!(
                    "{}:{}:{}:{}",
                    workspace_id, doc_id, lease.section_id, lease.agent_id
                ),
                expires_at: lease.expires_at,
                conflicts,
            })
        })
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
            title,
            head_seq: 0,
            etag: format!("doc:{doc_id}:0"),
        };
        self.doc_metadata.write().await.insert((workspace_id, doc_id), metadata);
        self.record_doc_snapshot(workspace_id, doc_id, 0, markdown).await;
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
        let mut history = self.doc_history.write().await;
        history
            .entry((workspace_id, doc_id))
            .or_default()
            .entry(seq)
            .or_insert_with(|| content_md.to_string());
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
                workspace_id: ws.workspace_id,
                name: ws.name.clone(),
                root_path: ws.root_path.clone(),
                doc_count: doc_counts.get(&ws.workspace_id).copied().unwrap_or(0),
                created_at: ws.created_at,
            })
            .collect();
        items.sort_by(|a, b| a.name.cmp(&b.name));

        let total = items.len();
        let items: Vec<WorkspaceListItem> = items.into_iter().skip(offset).take(limit).collect();

        WorkspaceListResult { items, total }
    }

    async fn workspace_open(&self, workspace_id: Uuid) -> Result<WorkspaceOpenResult, String> {
        let workspaces = self.workspaces.read().await;
        let ws = workspaces
            .get(&workspace_id)
            .ok_or_else(|| format!("workspace {} not found", workspace_id))?;

        let doc_metadata = self.doc_metadata.read().await;
        let doc_count = doc_metadata
            .keys()
            .filter(|(ws_id, _)| *ws_id == workspace_id)
            .count();

        Ok(WorkspaceOpenResult {
            workspace_id: ws.workspace_id,
            name: ws.name.clone(),
            root_path: ws.root_path.clone(),
            doc_count,
            created_at: ws.created_at,
        })
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

        // Create `.scriptum/` directory and default workspace.toml.
        let scriptum_dir = path.join(".scriptum");
        std::fs::create_dir_all(&scriptum_dir)
            .map_err(|e| format!("failed to create .scriptum directory: {e}"))?;

        let config = crate::config::WorkspaceConfig::default();
        config
            .save(path)
            .map_err(|e| format!("failed to write workspace.toml: {e}"))?;

        let workspace_id = Uuid::new_v4();
        let created_at = chrono::Utc::now();
        let info = WorkspaceInfo {
            workspace_id,
            name: trimmed_name.clone(),
            root_path: root_path.clone(),
            created_at,
        };
        self.workspaces.write().await.insert(workspace_id, info);

        Ok(WorkspaceCreateResult {
            workspace_id,
            name: trimmed_name,
            root_path,
            created_at,
        })
    }

    async fn read_doc(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        include_content: bool,
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

        let content_md = doc.get_text_string("content");
        let sections = parse_sections(&content_md);
        self.record_doc_snapshot(workspace_id, doc_id, metadata.head_seq, &content_md).await;
        let degraded = self.degraded_docs.read().await.contains(&doc_id);

        {
            let mut manager = self.doc_manager.write().await;
            let _ = manager.unsubscribe(doc_id);
        }

        DocReadResult { metadata, sections, content_md: include_content.then_some(content_md), degraded }
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

            if let Some(content_md) = params.content_md.as_deref() {
                let existing_len = doc.text_len("content");
                doc.replace_text("content", 0, existing_len, content_md);
            }

            if let Some(ops_value) = params.ops.as_ref() {
                let update_bytes = decode_doc_edit_ops(ops_value)?;
                doc.apply_update(&update_bytes)
                    .map_err(|error| format!("failed to apply Yjs ops: {error}"))?;
            }

            let updated_content = doc.get_text_string("content");
            let (result, updated_seq) = {
                let mut metadata = self.doc_metadata.write().await;
                let record = metadata
                    .entry((params.workspace_id, params.doc_id))
                    .or_insert_with(|| default_metadata(params.workspace_id, params.doc_id));
                record.head_seq = record.head_seq.saturating_add(1);
                record.etag = format!("doc:{}:{}", params.doc_id, record.head_seq);
                (
                    DocEditResult { etag: record.etag.clone(), head_seq: record.head_seq },
                    record.head_seq,
                )
            };
            self.record_doc_snapshot(params.workspace_id, params.doc_id, updated_seq, &updated_content)
                .await;

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

    async fn doc_tree(
        &self,
        workspace_id: Uuid,
        path_prefix: Option<&str>,
    ) -> DocTreeResult {
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
            return Err(format!(
                "limit must be between 1 and {}",
                DOC_SEARCH_MAX_LIMIT
            ));
        }

        let offset = decode_doc_search_cursor(params.cursor.as_deref())?;
        let metadata: Vec<DocMetadataRecord> = self
            .doc_metadata
            .read()
            .await
            .values()
            .filter(|record| record.workspace_id == params.workspace_id)
            .cloned()
            .collect();
        if metadata.is_empty() {
            return Ok(DocSearchResult { items: Vec::new(), total: 0, next_cursor: None });
        }

        let connection = rusqlite::Connection::open_in_memory()
            .map_err(|error| format!("failed to initialize in-memory search db: {error}"))?;
        let index = Fts5Index::new(&connection);
        index
            .ensure_schema()
            .map_err(|error| format!("failed to create search index schema: {error}"))?;

        let mut metadata_by_doc_id = HashMap::with_capacity(metadata.len());
        for record in metadata {
            let doc = {
                let mut manager = self.doc_manager.write().await;
                manager.subscribe_or_create(record.doc_id)
            };
            let content = doc.get_text_string("content");
            {
                let mut manager = self.doc_manager.write().await;
                let _ = manager.unsubscribe(record.doc_id);
            }

            index
                .upsert(&IndexEntry {
                    doc_id: record.doc_id.to_string(),
                    title: record.title.clone(),
                    content,
                })
                .map_err(|error| format!("failed to index doc {}: {error}", record.doc_id))?;
            metadata_by_doc_id.insert(record.doc_id.to_string(), record);
        }

        let hits = index
            .search(query, metadata_by_doc_id.len())
            .map_err(|error| format!("failed to search docs: {error}"))?;
        if offset > hits.len() {
            return Err(format!("cursor offset {offset} is out of range"));
        }

        let end = offset.saturating_add(params.limit).min(hits.len());
        let mut items = Vec::with_capacity(end.saturating_sub(offset));
        for hit in &hits[offset..end] {
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

        let next_cursor = (end < hits.len()).then(|| encode_doc_search_cursor(end));
        Ok(DocSearchResult {
            items,
            total: hits.len(),
            next_cursor,
        })
    }

    async fn doc_diff(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        from_seq: i64,
        to_seq: i64,
    ) -> Result<DocDiffResult, String> {
        if from_seq < 0 || to_seq < 0 {
            return Err("from_seq and to_seq must be >= 0".to_string());
        }
        if from_seq > to_seq {
            return Err(format!(
                "from_seq must be <= to_seq (from_seq={from_seq}, to_seq={to_seq})"
            ));
        }

        let head_seq = {
            let mut metadata = self.doc_metadata.write().await;
            metadata
                .entry((workspace_id, doc_id))
                .or_insert_with(|| default_metadata(workspace_id, doc_id))
                .head_seq
        };

        if from_seq > head_seq || to_seq > head_seq {
            return Err(format!(
                "requested sequence range [{from_seq}, {to_seq}] exceeds head_seq {head_seq}"
            ));
        }

        let mut snapshots = {
            let history = self.doc_history.read().await;
            let doc_history = history.get(&(workspace_id, doc_id));
            (
                doc_history.and_then(|h| h.get(&from_seq).cloned()),
                doc_history.and_then(|h| h.get(&to_seq).cloned()),
            )
        };

        if snapshots.0.is_none() || snapshots.1.is_none() {
            let doc = {
                let mut manager = self.doc_manager.write().await;
                manager.subscribe_or_create(doc_id)
            };
            let current_content = doc.get_text_string("content");
            self.record_doc_snapshot(workspace_id, doc_id, head_seq, &current_content).await;
            {
                let mut manager = self.doc_manager.write().await;
                let _ = manager.unsubscribe(doc_id);
            }

            let history = self.doc_history.read().await;
            if let Some(doc_history) = history.get(&(workspace_id, doc_id)) {
                snapshots.0 = snapshots.0.or_else(|| doc_history.get(&from_seq).cloned());
                snapshots.1 = snapshots.1.or_else(|| doc_history.get(&to_seq).cloned());
            }
        }

        let Some(from_content) = snapshots.0 else {
            return Err(format!(
                "sequence {from_seq} is unavailable for doc {doc_id} (head_seq={head_seq})"
            ));
        };
        let Some(to_content) = snapshots.1 else {
            return Err(format!(
                "sequence {to_seq} is unavailable for doc {doc_id} (head_seq={head_seq})"
            ));
        };

        Ok(DocDiffResult { patch_md: render_markdown_patch(&from_content, &to_content) })
    }

    async fn bundle_doc(&self, params: DocBundleParams) -> Result<DocBundleResult, String> {
        let doc = {
            let mut manager = self.doc_manager.write().await;
            manager.subscribe_or_create(params.doc_id)
        };
        let doc_id = params.doc_id;

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
                context.backlinks = Vec::new();
            }

            if include.contains(&DocBundleInclude::Comments) {
                context.comments = Vec::new();
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
            RpcError { code: INVALID_REQUEST, message: "Invalid Request".to_string(), data: None },
        );
    }

    dispatch_request(request, state).await
}

pub async fn dispatch_request(request: Request, state: &RpcServerState) -> Response {
    match request.method.as_str() {
        "rpc.ping" => Response::success(
            request.id,
            json!({
                "ok": true,
            }),
        ),
        "daemon.shutdown" => {
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
        "doc.read" => handle_doc_read(request, state).await,
        "doc.edit" => handle_doc_edit(request, state).await,
        "doc.bundle" => handle_doc_bundle(request, state).await,
        "doc.edit_section" => handle_doc_edit_section(request, state).await,
        "doc.sections" => handle_doc_sections(request, state).await,
        "doc.diff" => handle_doc_diff(request, state).await,
        "doc.search" => handle_doc_search(request, state).await,
        "doc.tree" => handle_doc_tree(request, state).await,
        "agent.whoami" => handle_agent_whoami(request, state),
        "agent.status" => handle_agent_status(request, state),
        "agent.conflicts" => handle_agent_conflicts(request, state),
        "agent.list" => handle_agent_list(request, state),
        "agent.claim" => handle_agent_claim(request, state),
        "workspace.list" => handle_workspace_list(request, state).await,
        "workspace.open" => handle_workspace_open(request, state).await,
        "workspace.create" => handle_workspace_create(request, state).await,
        "git.status" => handle_git_status(request, state),
        "git.sync" => handle_git_sync(request, state),
        "git.configure" => handle_git_configure(request, state),
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

    let result = state.read_doc(params.workspace_id, params.doc_id, params.include_content).await;
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
        return Err(invalid_params_response(
            request_id,
            "doc.bundle requires params".to_string(),
        ));
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
        Err(e) => Response::error(
            request.id,
            RpcError {
                code: INTERNAL_ERROR,
                message: e,
                data: None,
            },
        ),
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

    match state.doc_diff(params.workspace_id, params.doc_id, params.from_seq, params.to_seq).await {
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
        return Err(invalid_params_response(
            request_id,
            "doc.search requires params".to_string(),
        ));
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

    let result = state
        .doc_tree(params.workspace_id, params.path_prefix.as_deref())
        .await;
    Response::success(request.id, json!(result))
}

fn handle_agent_whoami(request: Request, state: &RpcServerState) -> Response {
    let result = AgentWhoamiResult {
        agent_id: (*state.agent_id).clone(),
        capabilities: vec![
            "agent.whoami".to_string(),
            "agent.status".to_string(),
            "agent.conflicts".to_string(),
            "agent.list".to_string(),
            "agent.claim".to_string(),
        ],
    };
    Response::success(request.id, json!(result))
}

fn handle_agent_status(request: Request, state: &RpcServerState) -> Response {
    let params = match parse_agent_status_params(request.params, request.id.clone()) {
        Ok(params) => params,
        Err(response) => return response,
    };

    match state.agent_status(params.workspace_id) {
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
        return Err(invalid_params_response(
            request_id,
            "agent.list requires params".to_string(),
        ));
    };

    serde_json::from_value::<AgentListParams>(params).map_err(|error| {
        invalid_params_response(
            request_id,
            format!("failed to decode agent.list params: {error}"),
        )
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
            if reason.contains("ttl_sec must be > 0") || reason.contains("section_id must not be empty") {
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
        return Err(invalid_params_response(
            request_id,
            "agent.claim requires params".to_string(),
        ));
    };

    serde_json::from_value::<AgentClaimParams>(params).map_err(|error| {
        invalid_params_response(
            request_id,
            format!("failed to decode agent.claim params: {error}"),
        )
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
        .filter_map(|(index, line)| {
            (index >= start_line && index < end_line).then_some(line)
        })
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
        current_parent = sections_by_id.get(parent_id).and_then(|section| section.parent_id.as_deref());
    }
    false
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

fn count_serialized_tokens_with<T, F>(
    value: &T,
    token_counter: &F,
) -> Result<usize, String>
where
    T: Serialize,
    F: Fn(&str) -> Result<usize, String>,
{
    let serialized =
        serde_json::to_string(value).map_err(|error| format!("failed to serialize bundle data: {error}"))?;
    token_counter(&serialized)
}

fn count_tokens_cl100k(value: &str) -> Result<usize, String> {
    let tokenizer = cl100k_tokenizer()?;
    Ok(tokenizer.encode_with_special_tokens(value).len())
}

fn cl100k_tokenizer() -> Result<&'static CoreBPE, String> {
    static TOKENIZER: OnceLock<Result<CoreBPE, String>> = OnceLock::new();
    let tokenizer = TOKENIZER.get_or_init(|| tiktoken_rs::cl100k_base().map_err(|error| error.to_string()));

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
                    "doc.edit `ops` object must include `payload_b64` (or `base64`)".to_string(),
                );
            };
            let Some(payload_b64) = payload_b64_value.as_str() else {
                return Err("doc.edit `ops.payload_b64` must be a base64 string".to_string());
            };
            decode_doc_edit_ops_base64(payload_b64)
        }
        _ => Err(
            "doc.edit `ops` must be a base64 string, byte array, or object with `payload_b64`"
                .to_string(),
        ),
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
    let as_text =
        String::from_utf8(decoded).map_err(|error| format!("cursor is not valid utf-8: {error}"))?;
    as_text
        .parse::<usize>()
        .map_err(|error| format!("cursor is not a valid offset: {error}"))
}

fn encode_doc_search_cursor(offset: usize) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(offset.to_string())
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
    #[serde(skip_serializing_if = "Option::is_none")]
    last_sync_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GitSyncAction {
    Commit { message: String },
    CommitAndPush { message: String },
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

    match state.workspace_open(params.workspace_id).await {
        Ok(result) => Response::success(request.id, json!(result)),
        Err(reason) => Response::error(
            request.id,
            RpcError {
                code: INTERNAL_ERROR,
                message: reason,
                data: None,
            },
        ),
    }
}

async fn handle_workspace_create(request: Request, state: &RpcServerState) -> Response {
    let Some(params) = request.params else {
        return invalid_params_response(
            request.id,
            "workspace.create requires params".to_string(),
        );
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
            if reason.contains("must not be empty") || reason.contains("must be an absolute path") {
                invalid_params_response(request.id, reason)
            } else {
                Response::error(
                    request.id,
                    RpcError {
                        code: INTERNAL_ERROR,
                        message: reason,
                        data: None,
                    },
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

fn handle_git_sync(request: Request, state: &RpcServerState) -> Response {
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

    match git.sync(params.action) {
        Ok(job_id) => Response::success(request.id, json!({ "job_id": job_id })),
        Err(e) => Response::error(
            request.id,
            RpcError {
                code: INTERNAL_ERROR,
                message: format!("git sync failed: {e}"),
                data: None,
            },
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use base64::Engine;
    use scriptum_common::protocol::jsonrpc::{Request, RequestId, INTERNAL_ERROR, INVALID_PARAMS};
    use scriptum_common::types::Section;
    use serde_json::json;
    use tokio::sync::broadcast;
    use uuid::Uuid;

    use crate::engine::ydoc::YDoc;

    use super::{
        apply_bundle_token_budget_with, dispatch_request, BacklinkContext, CommentThreadContext,
        DocBundleContext, GitOps, GitStatusInfo, GitSyncAction, GitSyncPolicy, RpcServerState,
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

        fn sync(&self, action: GitSyncAction) -> Result<Uuid, String> {
            let label = match &action {
                GitSyncAction::Commit { message } => format!("commit:{message}"),
                GitSyncAction::CommitAndPush { message } => format!("commit_and_push:{message}"),
            };
            self.sync_calls.lock().unwrap().push(label);
            self.sync_result.lock().unwrap().clone()
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

    fn encoded_doc_ops(content: &str) -> String {
        let source = YDoc::new();
        source.insert_text("content", 0, content);
        base64::engine::general_purpose::STANDARD.encode(source.encode_state())
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
        assert_eq!(result["metadata"]["workspace_id"], json!(workspace_id));
        assert_eq!(result["metadata"]["doc_id"], json!(doc_id));
        assert_eq!(result["metadata"]["path"], json!("docs/readme.md"));
        assert_eq!(result["metadata"]["title"], json!("Readme"));
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

        let read = state.read_doc(workspace_id, doc_id, true).await;
        assert_eq!(read.content_md, Some("# After\nnew".to_string()));
        assert_eq!(read.metadata.head_seq, 1);
        assert_eq!(read.metadata.etag, format!("doc:{doc_id}:1"));
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
        let read = state.read_doc(workspace_id, doc_id, true).await;
        assert_eq!(read.content_md, Some("inserted via ops".to_string()));
        assert_eq!(read.metadata.head_seq, 1);
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

        let read = state.read_doc(workspace_id, doc_id, true).await;
        assert_eq!(read.content_md, Some("unchanged".to_string()));
        assert_eq!(read.metadata.head_seq, 0);
        assert_eq!(read.metadata.etag, format!("doc:{doc_id}:0"));
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
        state.seed_doc(workspace_id, doc_id, "docs/history.md", "History", "# Title\nold line\n").await;

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
        assert!(edit_1_response.error.is_none(), "expected first edit to succeed: {edit_1_response:?}");

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
        assert!(edit_2_response.error.is_none(), "expected second edit to succeed: {edit_2_response:?}");

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

        assert!(patch.starts_with("```diff\n"));
        assert!(patch.contains("-old line"));
        assert!(patch.contains("+new line"));
        assert!(patch.contains("+extra line"));
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
    async fn doc_bundle_returns_section_content_and_context() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        let markdown = "# Root\nroot body\n\n## Child\nchild body\n\n### Grandchild\ndeep body\n";
        state.seed_doc(workspace_id, doc_id, "docs/readme.md", "Readme", markdown).await;

        let request = Request::new(
            "doc.bundle",
            Some(json!({
                "workspace_id": workspace_id,
                "doc_id": doc_id,
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
        assert_eq!(result["context"]["backlinks"], json!([]));
        assert_eq!(result["context"]["comments"], json!([]));
        assert!(result["tokens_used"].as_u64().expect("tokens_used should be numeric") > 0);
    }

    #[tokio::test]
    async fn doc_bundle_rejects_unknown_section_id() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();
        state.seed_doc(workspace_id, doc_id, "docs/readme.md", "Readme", "# Root\n")
            .await;

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
                thread_id: "thread-1".to_string(),
                section_id: "root/child".to_string(),
                excerpt: "M".repeat(120),
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
        let section_only_budget =
            apply_bundle_token_budget_with(section_content, &mut section_only_context, None, &token_counter)
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
        let capabilities = result["capabilities"]
            .as_array()
            .expect("capabilities should be an array");
        assert!(capabilities.contains(&json!("agent.claim")));
        assert!(capabilities.contains(&json!("agent.status")));
    }

    #[tokio::test]
    async fn agent_claim_returns_lease_and_conflicts() {
        let state = RpcServerState::default().with_agent_identity("claude-1");
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();

        claim_section(&state, 61, workspace_id, doc_id, "root/auth", "claude-1", "exclusive")
            .await;

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
        let lease_id = result["lease_id"]
            .as_str()
            .expect("lease_id should be a string");
        assert!(lease_id.contains(&workspace_id.to_string()));
        assert!(lease_id.contains(&doc_id.to_string()));
        assert!(lease_id.contains("root/auth"));
        assert!(lease_id.contains("copilot-1"));

        let conflicts = result["conflicts"]
            .as_array()
            .expect("conflicts should be an array");
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

        claim_section(&state, 63, workspace_id, doc_id, "root/auth", "claude-1", "exclusive")
            .await;
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
        let sessions = result["active_sessions"]
            .as_array()
            .expect("active_sessions should be an array");
        assert_eq!(sessions.len(), 2);

        let mut sections_by_agent = HashMap::new();
        for session in sessions {
            let agent_id = session["agent_id"]
                .as_str()
                .expect("agent_id should be a string")
                .to_string();
            let active_sections = session["active_sections"]
                .as_u64()
                .expect("active_sections should be numeric") as u32;
            sections_by_agent.insert(agent_id, active_sections);
        }
        assert_eq!(sections_by_agent.get("claude-1"), Some(&2));
        assert_eq!(sections_by_agent.get("copilot-1"), Some(&1));
    }

    #[tokio::test]
    async fn agent_conflicts_returns_overlapping_section_items() {
        let state = RpcServerState::default();
        let workspace_id = Uuid::new_v4();
        let doc_id = Uuid::new_v4();

        claim_section(&state, 67, workspace_id, doc_id, "root/auth", "claude-1", "exclusive")
            .await;
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
        let items = result["items"]
            .as_array()
            .expect("items should be an array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["section"]["id"], "root/auth");
        assert_eq!(items[0]["severity"], "info");
        let editors = items[0]["editors"]
            .as_array()
            .expect("editors should be an array");
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

        claim_section(&state, 71, workspace_id, doc_id, "root/auth", "claude-1", "exclusive")
            .await;
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
        let items = result["items"]
            .as_array()
            .expect("items should be an array");
        assert_eq!(items.len(), 2);

        let mut sections_by_agent = HashMap::new();
        for item in items {
            sections_by_agent.insert(
                item["agent_id"].as_str().expect("agent_id should be a string").to_string(),
                item["active_sections"]
                    .as_u64()
                    .expect("active_sections should be numeric") as u32,
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
        assert_eq!(calls[0], "commit:docs: update");
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
        assert_eq!(calls[0], "commit_and_push:feat: add X");
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
        let request = Request::new(
            "git.sync",
            Some(json!({ "action": "bad" })),
            RequestId::Number(24),
        );
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
            last_sync_at: None,
        }));
        let state = state_with_git(mock);
        let request = Request::new("git.status", None, RequestId::Number(40));
        let response = dispatch_request(request, &state).await;

        assert!(response.error.is_none(), "expected success: {response:?}");
        let result = response.result.expect("result should be populated");
        assert_eq!(result["dirty"], false);
        assert_eq!(result["policy"], "disabled");
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

        let request = Request::new(
            "doc.tree",
            Some(json!({ "workspace_id": ws })),
            RequestId::Number(210),
        );
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

        let request = Request::new(
            "doc.tree",
            Some(json!({ "workspace_id": ws_a })),
            RequestId::Number(213),
        );
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
        assert_eq!(first_items[0]["path"], json!("docs/alpha.md"));
        assert_eq!(first_items[0]["title"], json!("Alpha"));
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
        assert_eq!(second_items[0]["path"], json!("docs/beta.md"));
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
    async fn doc_search_rejects_invalid_cursor() {
        let state = RpcServerState::default();
        let request = Request::new(
            "doc.search",
            Some(json!({
                "workspace_id": Uuid::new_v4(),
                "q": "Scriptum",
                "cursor": "not-base64"
            })),
            RequestId::Number(218),
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
        assert_eq!(result["total"], 2);

        // Sorted by name: Alpha before Beta.
        assert_eq!(items[0]["name"], "Alpha");
        assert_eq!(items[0]["doc_count"], 1);
        assert_eq!(items[1]["name"], "Beta");
        assert_eq!(items[1]["doc_count"], 0);
    }

    #[tokio::test]
    async fn workspace_list_respects_pagination() {
        let state = RpcServerState::default();
        for i in 0..5 {
            state.seed_workspace(Uuid::new_v4(), format!("WS-{i:02}"), format!("/tmp/ws-{i}")).await;
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
        assert_eq!(result["name"], "New Project");
        assert_eq!(result["root_path"], json!(root));
        assert!(result["workspace_id"].as_str().is_some());
        assert!(result["created_at"].as_str().is_some());

        // Verify .scriptum/workspace.toml was created on disk.
        let toml_path = tmp.path().join(".scriptum").join("workspace.toml");
        assert!(toml_path.exists(), ".scriptum/workspace.toml should exist");

        // Verify workspace is now listed.
        let list_req = Request::new("workspace.list", None, RequestId::Number(121));
        let list_resp = dispatch_request(list_req, &state).await;
        let list_result = list_resp.result.expect("result");
        assert_eq!(list_result["total"], 1);
        assert_eq!(list_result["items"][0]["name"], "New Project");
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
}
