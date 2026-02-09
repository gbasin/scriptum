// RPC method name constants — derived from contracts/jsonrpc-methods.json.

// ── Daemon-internal ────────────────────────────────────────────────
pub const RPC_PING: &str = "rpc.ping";
pub const DAEMON_SHUTDOWN: &str = "daemon.shutdown";

// ── Document ───────────────────────────────────────────────────────
pub const DOC_READ: &str = "doc.read";
pub const DOC_CREATE: &str = "doc.create";
pub const DOC_EDIT: &str = "doc.edit";
pub const DOC_EDIT_SECTION: &str = "doc.edit_section";
pub const DOC_BUNDLE: &str = "doc.bundle";
pub const DOC_SECTIONS: &str = "doc.sections";
pub const DOC_DIFF: &str = "doc.diff";
pub const DOC_HISTORY: &str = "doc.history";
pub const DOC_SEARCH: &str = "doc.search";
pub const DOC_TREE: &str = "doc.tree";

// ── Agent ──────────────────────────────────────────────────────────
pub const AGENT_WHOAMI: &str = "agent.whoami";
pub const AGENT_STATUS: &str = "agent.status";
pub const AGENT_CONFLICTS: &str = "agent.conflicts";
pub const AGENT_LIST: &str = "agent.list";
pub const AGENT_CLAIM: &str = "agent.claim";

// ── Workspace ──────────────────────────────────────────────────────
pub const WORKSPACE_LIST: &str = "workspace.list";
pub const WORKSPACE_OPEN: &str = "workspace.open";
pub const WORKSPACE_CREATE: &str = "workspace.create";

// ── Git ────────────────────────────────────────────────────────────
pub const GIT_STATUS: &str = "git.status";
pub const GIT_SYNC: &str = "git.sync";
pub const GIT_CONFIGURE: &str = "git.configure";

/// All methods the daemon currently dispatches.
pub const IMPLEMENTED_METHODS: &[&str] = &[
    RPC_PING,
    DAEMON_SHUTDOWN,
    DOC_READ,
    DOC_CREATE,
    DOC_EDIT,
    DOC_EDIT_SECTION,
    DOC_BUNDLE,
    DOC_SECTIONS,
    DOC_DIFF,
    DOC_HISTORY,
    DOC_SEARCH,
    DOC_TREE,
    AGENT_WHOAMI,
    AGENT_STATUS,
    AGENT_CONFLICTS,
    AGENT_LIST,
    AGENT_CLAIM,
    WORKSPACE_LIST,
    WORKSPACE_OPEN,
    WORKSPACE_CREATE,
    GIT_STATUS,
    GIT_SYNC,
    GIT_CONFIGURE,
];

/// Methods acknowledged in the contract as planned but not yet implemented.
pub const PLANNED_METHODS: &[&str] = &[
    "doc.read_section",
    "doc.peek",
    "doc.blame",
    "workspace.ls",
    "workspace.diff",
    "workspace.search",
    "lease.claim",
];
