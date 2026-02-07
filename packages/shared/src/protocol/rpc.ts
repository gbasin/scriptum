export type JsonRpcVersion = "2.0";

export type JsonRpcId = string | number | null;

export interface JsonRpcError {
  code: number;
  message: string;
  data?: unknown;
}

export interface JsonRpcErrorResponse {
  jsonrpc: JsonRpcVersion;
  id: JsonRpcId;
  error: JsonRpcError;
}

export type RpcBundleInclude =
  | "parents"
  | "children"
  | "backlinks"
  | "comments";

export type AgentClaimMode = "exclusive" | "shared";

export type GitSyncMode = "commit" | "commit_and_push";

export type GitSyncJobStatus = "queued" | "running" | "completed" | "failed";

export type YjsOps = unknown;

export type GitSyncPolicy = Record<string, unknown>;

export interface RpcWorkspace {
  id: string;
  slug: string;
  name: string;
  role?: string;
  created_at: string;
  updated_at: string;
  etag: string;
}

export interface RpcDocument {
  id: string;
  workspace_id: string;
  path: string;
  title: string;
  tags?: string[];
  head_seq: number;
  etag: string;
  archived_at?: string | null;
  deleted_at?: string | null;
  created_at: string;
  updated_at: string;
}

export interface RpcSection {
  id: string;
  parent_id?: string | null;
  heading: string;
  level: number;
  start_line: number;
  end_line: number;
}

export interface RpcCommentThread {
  id: string;
  workspace_id: string;
  doc_id: string;
  section_id?: string | null;
  status: "open" | "resolved";
  version: number;
  created_at: string;
  resolved_at?: string | null;
}

export interface RpcAgentSession {
  agent_id: string;
  workspace_id: string;
  last_seen_at: string;
  active_sections: number;
}

export interface RpcOverlapEditor {
  name: string;
  editor_type: "human" | "agent";
  cursor_offset: number;
  last_edit_at: string;
}

export interface RpcSectionOverlap {
  section: RpcSection;
  editors: RpcOverlapEditor[];
  severity: "info" | "warning";
}

export interface WorkspaceListParams {
  limit?: number;
  cursor?: string;
}

export interface WorkspaceListResult {
  items: RpcWorkspace[];
  next_cursor: string | null;
}

export interface WorkspaceOpenParams {
  workspace_id: string;
}

export interface WorkspaceOpenResult {
  workspace: RpcWorkspace;
  root_path: string;
}

export interface WorkspaceCreateParams {
  name: string;
  root_path: string;
}

export interface WorkspaceCreateResult {
  workspace: RpcWorkspace;
}

export interface DocReadParams {
  workspace_id: string;
  doc_id: string;
  include_content?: boolean;
}

export interface DocReadResult {
  document: RpcDocument;
  content_md?: string;
  sections: RpcSection[];
}

export interface DocEditParams {
  workspace_id: string;
  doc_id: string;
  client_update_id: string;
  ops?: YjsOps;
  content_md?: string;
  if_etag?: string;
  agent_id?: string;
}

export interface DocEditResult {
  etag: string;
  head_seq: number;
}

export interface DocSectionsParams {
  workspace_id: string;
  doc_id: string;
}

export interface DocSectionsResult {
  sections: RpcSection[];
}

export interface DocTreeParams {
  workspace_id: string;
  path_prefix?: string;
}

export interface DocTreeItem {
  path: string;
  doc_id: string;
  title: string;
}

export interface DocTreeResult {
  items: DocTreeItem[];
}

export interface DocSearchParams {
  workspace_id: string;
  q: string;
  limit?: number;
  cursor?: string;
}

export interface DocSearchItem {
  doc_id: string;
  path: string;
  title: string;
  snippet: string;
  score: number;
}

export interface DocSearchResult {
  items: DocSearchItem[];
  next_cursor: string | null;
}

export interface DocDiffParams {
  workspace_id: string;
  doc_id: string;
  from_seq: number;
  to_seq: number;
}

export interface DocDiffResult {
  patch_md: string;
}

export type AgentWhoamiParams = {};

export interface AgentWhoamiResult {
  agent_id: string;
  capabilities: string[];
}

export interface AgentStatusParams {
  workspace_id: string;
}

export interface AgentStatusResult {
  active_sessions: RpcAgentSession[];
}

export interface AgentConflictsParams {
  workspace_id: string;
  doc_id?: string;
}

export interface AgentConflictsResult {
  items: RpcSectionOverlap[];
}

export interface AgentListParams {
  workspace_id: string;
}

export interface AgentListItem {
  agent_id: string;
  last_seen_at: string;
  active_sections: number;
}

export interface AgentListResult {
  items: AgentListItem[];
}

export interface AgentClaimParams {
  workspace_id: string;
  doc_id: string;
  section_id: string;
  ttl_sec: number;
  mode: AgentClaimMode;
  note?: string;
}

export interface AgentClaimConflict {
  agent_id: string;
  section_id: string;
}

export interface AgentClaimResult {
  lease_id: string;
  expires_at: string;
  conflicts: AgentClaimConflict[];
}

export interface DocBundleParams {
  workspace_id: string;
  doc_id: string;
  section_id?: string;
  include: RpcBundleInclude[];
  token_budget?: number;
}

export interface DocBundleBacklink {
  doc_id: string;
  path: string;
  snippet: string;
}

export interface DocBundleContext {
  parents: RpcSection[];
  children: RpcSection[];
  backlinks: DocBundleBacklink[];
  comments: RpcCommentThread[];
}

export interface DocBundleResult {
  section_content: string;
  context: DocBundleContext;
  tokens_used: number;
}

export interface GitStatusParams {
  workspace_id: string;
}

export interface GitStatusResult {
  branch: string;
  dirty: boolean;
  ahead: number;
  behind: number;
  last_sync_at: string | null;
}

export interface GitSyncParams {
  workspace_id: string;
  mode: GitSyncMode;
  agent_id?: string;
}

export interface GitSyncResult {
  job_id: string;
  status: GitSyncJobStatus;
}

export interface GitConfigureParams {
  workspace_id: string;
  policy: GitSyncPolicy;
}

export interface GitConfigureResult {
  policy: GitSyncPolicy;
}

export interface RpcParamsMap {
  "workspace.list": WorkspaceListParams;
  "workspace.open": WorkspaceOpenParams;
  "workspace.create": WorkspaceCreateParams;
  "doc.read": DocReadParams;
  "doc.edit": DocEditParams;
  "doc.sections": DocSectionsParams;
  "doc.tree": DocTreeParams;
  "doc.search": DocSearchParams;
  "doc.diff": DocDiffParams;
  "agent.whoami": AgentWhoamiParams;
  "agent.status": AgentStatusParams;
  "agent.conflicts": AgentConflictsParams;
  "agent.list": AgentListParams;
  "agent.claim": AgentClaimParams;
  "doc.bundle": DocBundleParams;
  "git.status": GitStatusParams;
  "git.sync": GitSyncParams;
  "git.configure": GitConfigureParams;
}

export interface RpcResultMap {
  "workspace.list": WorkspaceListResult;
  "workspace.open": WorkspaceOpenResult;
  "workspace.create": WorkspaceCreateResult;
  "doc.read": DocReadResult;
  "doc.edit": DocEditResult;
  "doc.sections": DocSectionsResult;
  "doc.tree": DocTreeResult;
  "doc.search": DocSearchResult;
  "doc.diff": DocDiffResult;
  "agent.whoami": AgentWhoamiResult;
  "agent.status": AgentStatusResult;
  "agent.conflicts": AgentConflictsResult;
  "agent.list": AgentListResult;
  "agent.claim": AgentClaimResult;
  "doc.bundle": DocBundleResult;
  "git.status": GitStatusResult;
  "git.sync": GitSyncResult;
  "git.configure": GitConfigureResult;
}

export type RpcMethod = keyof RpcParamsMap;

export interface JsonRpcRequest<M extends RpcMethod = RpcMethod> {
  jsonrpc: JsonRpcVersion;
  id: JsonRpcId;
  method: M;
  params: RpcParamsMap[M];
}

export interface JsonRpcSuccessResponse<M extends RpcMethod = RpcMethod> {
  jsonrpc: JsonRpcVersion;
  id: JsonRpcId;
  result: RpcResultMap[M];
}

export type JsonRpcResponse<M extends RpcMethod = RpcMethod> =
  | JsonRpcSuccessResponse<M>
  | JsonRpcErrorResponse;
