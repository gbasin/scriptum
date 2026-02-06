use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use crate::engine::{doc_manager::DocManager, ydoc::YDoc};
use crate::git::worker::{CommandExecutor, GitWorker, ProcessCommandExecutor};
use scriptum_common::protocol::jsonrpc::{
    Request, RequestId, Response, RpcError, INTERNAL_ERROR, INVALID_PARAMS, INVALID_REQUEST,
    METHOD_NOT_FOUND, PARSE_ERROR,
};
use scriptum_common::section::parser::parse_sections;
use scriptum_common::types::Section;
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
    shutdown_notifier: Option<broadcast::Sender<()>>,
    git_state: Option<Arc<dyn GitOps + Send + Sync>>,
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

impl Default for RpcServerState {
    fn default() -> Self {
        Self {
            doc_manager: Arc::new(RwLock::new(DocManager::default())),
            doc_metadata: Arc::new(RwLock::new(HashMap::new())),
            shutdown_notifier: None,
            git_state: None,
        }
    }
}

impl RpcServerState {
    /// Expose doc_manager for integration tests (e.g., CRDT sync verification).
    pub fn doc_manager_for_test(&self) -> &Arc<RwLock<DocManager>> {
        &self.doc_manager
    }

    pub fn with_shutdown_notifier(mut self, shutdown_notifier: broadcast::Sender<()>) -> Self {
        self.shutdown_notifier = Some(shutdown_notifier);
        self
    }

    pub fn with_git_state<E: CommandExecutor + 'static>(mut self, git: GitState<E>) -> Self {
        self.git_state = Some(Arc::new(git));
        self
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

        {
            let mut manager = self.doc_manager.write().await;
            let _ = manager.unsubscribe(doc_id);
        }

        DocReadResult { metadata, sections, content_md: include_content.then_some(content_md) }
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
        "doc.bundle" => handle_doc_bundle(request, state).await,
        "doc.edit_section" => handle_doc_edit_section(request, state).await,
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
    use std::sync::{Arc, Mutex};

    use scriptum_common::protocol::jsonrpc::{Request, RequestId, INTERNAL_ERROR, INVALID_PARAMS};
    use scriptum_common::types::Section;
    use serde_json::json;
    use tokio::sync::broadcast;
    use uuid::Uuid;

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
}
