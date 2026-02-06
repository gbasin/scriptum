# Scriptum - Technical Architecture

## System Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              CLIENTS                                        │
│                                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌───────────────┐  │
│  │  Desktop App  │  │   Web App    │  │  CLI Tool    │  │  MCP Server   │  │
│  │  (Tauri+React)│  │   (React)    │  │  (Rust)      │  │  (TypeScript) │  │
│  │  CodeMirror 6 │  │  CodeMirror 6│  │              │  │  (for agents) │  │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  └───────┬───────┘  │
│         │                  │                  │                   │          │
│         └──────────────────┼──────────────────┼───────────────────┘          │
│                            │                  │                              │
│                     ┌──────▼──────────────────▼──────┐                      │
│                     │        CRDT Engine (Yjs)        │                      │
│                     │   Shared document state layer   │                      │
│                     └──────────────┬─────────────────┘                      │
│                                    │                                        │
│         ┌──────────────────────────┼──────────────────────────┐             │
│         │                          │                          │             │
│  ┌──────▼───────┐  ┌──────────────▼──────────────┐  ┌────────▼────────┐   │
│  │ File Watcher  │  │    Section Awareness Layer   │  │  Git Sync       │   │
│  │ (local FS)    │  │  (markdown structure parser)  │  │  (AI commits)   │   │
│  └──────────────┘  └─────────────────────────────┘  └─────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
                                     │
                            ┌────────▼────────┐
                            │   Sync Layer     │
                            │                  │
                            │  ┌────────────┐  │
                            │  │  WebRTC    │  │  ← P2P (default)
                            │  │  (y-webrtc)│  │
                            │  └────────────┘  │
                            │                  │
                            │  ┌────────────┐  │
                            │  │  Relay     │  │  ← Required (sync + persistence)
                            │  │  Server    │  │
                            │  └────────────┘  │
                            │                  │
                            └──────────────────┘
```

---

## Tech Stack

### Desktop App
| Component | Technology | Rationale |
|-----------|-----------|-----------|
| Shell | **Tauri 2.0** | ~10MB binary, Rust backend, native performance, file system access |
| Frontend | **React 19 + TypeScript** | Ecosystem, component libraries, developer familiarity |
| Editor | **CodeMirror 6** + custom Live Preview extension | Y.Text native, exact formatting preservation, MIT licensed, Obsidian-style live preview |
| Yjs Binding | **y-codemirror.next** | Sync, remote cursors, shared undo/redo |
| CRDT | **Yjs** | Battle-tested, CodeMirror 6 integration, efficient binary encoding |
| Styling | **Tailwind CSS 4** | Utility-first, consistent design, fast iteration |
| State | **Zustand** | Lightweight, works well with Yjs reactive updates |
| Build | **Vite** | Fast HMR, Tauri integration |

### Web App
| Component | Technology | Rationale |
|-----------|-----------|-----------|
| Framework | **React 19 + TypeScript** | Shared code with desktop app |
| Editor | **CodeMirror 6** + custom Live Preview extension (same as desktop) | Identical editing experience, MIT licensed |
| Yjs Binding | **y-codemirror.next** | Sync, remote cursors, shared undo/redo |
| Hosting | **Cloudflare Pages** or self-hosted | Edge-deployed, fast globally |
| Auth | **GitHub OAuth** + optional email/password | Primary audience is dev teams |

### CLI Tool
| Component | Technology | Rationale |
|-----------|-----------|-----------|
| Language | **Rust** | Fast, single binary, shares code with Tauri backend |
| CRDT | **y-crdt** (Rust Yjs bindings) | Same CRDT engine as frontend |
| Markdown | **pulldown-cmark** | Fast, CommonMark-compliant markdown parser |

### MCP Server
| Component | Technology | Rationale |
|-----------|-----------|-----------|
| Language | **TypeScript** (Node.js) | MCP SDK is TypeScript-first |
| Protocol | **MCP (Model Context Protocol)** | Native Claude Code / Cursor integration |
| Transport | **stdio** | Standard MCP transport for local agents |
| Daemon IPC | **JSON-RPC over Unix socket / named pipe** | Communicates with daemon (Rust) via the same socket the CLI uses |

### Relay Server
| Component | Technology | Rationale |
|-----------|-----------|-----------|
| Language | **Rust** (Axum) | Performance, memory safety, WebSocket handling |
| Protocol | **WebSocket** | Yjs sync protocol over WebSocket |
| Auth | **JWT tokens** | Workspace-scoped access tokens |
| Storage | **SQLite** (single database) | All relay metadata: workspaces, users, invites, document metadata |
| CRDT Storage | **File system** | Yjs binary updates stored as append-only files |

### Git Sync
| Component | Technology | Rationale |
|-----------|-----------|-----------|
| Git ops | **gitoxide** (Rust) or **libgit2** | Fast, no git CLI dependency |
| AI commits | **Claude API** | Generate commit messages from diffs |
| Scheduling | **Built into daemon** | Timer-based, triggers on save/idle |

---

## Core Architecture Details

### 1. CRDT Engine (Yjs)

Every document is a Yjs `Doc` containing:

```typescript
// Yjs document structure
const ydoc = new Y.Doc()

// The markdown content as a collaborative text type
const ytext = ydoc.getText('content')

// Document metadata (title, tags, etc.)
const ymeta = ydoc.getMap('meta')

// Awareness (cursors, presence)
const awareness = new awarenessProtocol.Awareness(ydoc)
```

**Why Yjs over Automerge?**
- CodeMirror 6 has mature Yjs integration (y-codemirror.next)
- y-webrtc and y-websocket are mature
- Smaller wire format (important for real-time)
- Larger ecosystem of providers and bindings
- y-crdt (Rust) bindings for the CLI/backend

### 2. Section Awareness Layer

*(Design informed by [Niwa](https://github.com/secemp9/niwa)'s heading-based document tree and per-section conflict detection)*

A lightweight overlay that parses markdown structure and provides section-level intelligence. Unlike Niwa (which uses sections as the unit of conflict resolution), Scriptum uses sections as the unit of **awareness** - the CRDT handles the actual merging.

**Why this hybrid?** Pure CRDT merges can produce surprising results in prose (e.g., two people rewriting the same paragraph get both versions interleaved). Section awareness lets us warn users/agents *before* this happens, without blocking edits.

```typescript
interface SectionAwareness {
  // Parse document into heading-based section tree (like Niwa's node tree)
  parseSections(markdown: string): Section[]

  // Track which editors are in which sections (cursor position → section)
  getEditorSections(awareness: Awareness): Map<clientId, Section>

  // Detect when multiple editors are in the same section
  getSectionOverlaps(): SectionOverlap[]

  // Non-blocking callbacks when overlap detected
  onSectionOverlap(callback: (overlap: SectionOverlap) => void): void

  // Get per-section edit attribution history
  getSectionHistory(sectionId: string): SectionEdit[]
}

interface Section {
  id: string              // Stable ID: "h2:authentication" or "h2_3" (Niwa-style fallback)
  heading: string         // "## Authentication"
  level: number           // 2
  startOffset: number     // Character offset in Yjs text
  endOffset: number       // Character offset of next section
  parentId: string        // Parent section ID (builds a tree, like Niwa)
  children: string[]      // Child section IDs
  lastEditedBy: string    // Most recent editor (human or agent name)
  lastEditedAt: Date
  editCount: number       // Total edits to this section
}

interface SectionOverlap {
  section: Section
  editors: Array<{
    name: string           // Agent/user name
    type: 'human' | 'agent'
    cursorOffset: number   // Where they're editing within the section
    lastEditAt: Date
  }>
  severity: 'info' | 'warning'  // info = same section, warning = same paragraph
}

interface SectionEdit {
  editorName: string
  editorType: 'human' | 'agent'
  timestamp: Date
  summary?: string         // From --summary flag (agents) or auto-generated
  characterDelta: number   // +/- characters changed
}
```

**Section parsing approach** (borrowed from Niwa's markdown-it-py strategy):
- Parse markdown AST to extract heading tokens with line positions
- Build a tree: headings at level N are children of the nearest preceding heading at level N-1
- Content between headings belongs to the preceding heading's section
- Section IDs are stable across edits when possible (derived from heading text slug), with Niwa-style `h{level}_{index}` fallback for duplicate headings
- Re-parse on every CRDT update (fast - just heading extraction, not full render)

**How it integrates with Yjs:**
```
Yjs text update arrives
    │
    ▼
Re-parse section boundaries from updated markdown
    │
    ▼
Diff section tree against previous parse
    │
    ├── New section? → Track it
    ├── Removed section? → Archive attribution
    ├── Content changed? → Update lastEditedBy from Yjs awareness origin
    └── Multiple editors in same section? → Emit SectionOverlap event
```

This layer does NOT prevent concurrent edits (CRDT handles merging). It provides **awareness** - subtle UI indicators when two people are editing the same section, and richer attribution for the agent CLI.

### 3. File Watcher & Local Sync

> **Ownership**: The file watcher runs inside the daemon (Rust). The TypeScript-style
> code examples below are illustrative pseudocode showing the algorithm, not the
> actual implementation language. The real implementation is in Rust.

```
┌─────────────────────────────────────────────────────────────┐
│                    File Watcher Pipeline                      │
│                                                               │
│  FS Event (create/modify/delete)                              │
│       │                                                       │
│       ▼                                                       │
│  Debounce (100ms)                                             │
│       │                                                       │
│       ▼                                                       │
│  Read file content + compute hash                             │
│       │                                                       │
│       ▼                                                       │
│  Compare hash with last known CRDT state                      │
│       │                                                       │
│  ┌────┴────┐                                                  │
│  │ Changed │                                                  │
│  └────┬────┘                                                  │
│       │                                                       │
│       ▼                                                       │
│  Compute diff (CRDT state → new file content)                 │
│       │                                                       │
│       ▼                                                       │
│  Apply diff as Yjs operations                                 │
│       │                                                       │
│       ▼                                                       │
│  Yjs propagates to all connected peers                        │
│       │                                                       │
│       ▼                                                       │
│  (Do NOT write back to file - we just read from it)           │
│                                                               │
│  ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─  │
│                                                               │
│  Remote CRDT update received                                  │
│       │                                                       │
│       ▼                                                       │
│  Render Yjs state to markdown string                          │
│       │                                                       │
│       ▼                                                       │
│  Compare with current file on disk                            │
│       │                                                       │
│  ┌────┴────┐                                                  │
│  │ Changed │                                                  │
│  └────┬────┘                                                  │
│       │                                                       │
│       ▼                                                       │
│  Write to file (with watcher temporarily paused for this)     │
│                                                               │
└─────────────────────────────────────────────────────────────┘
```

**Key details:**
- **Debouncing**: 100ms debounce on file events to batch rapid saves
- **Hash tracking**: SHA-256 hash of file content to detect actual changes vs. no-op saves
- **Watcher pausing**: When writing remote changes to disk, temporarily ignore file events to prevent feedback loops
- **Diff algorithm**: Uses `diff-match-patch` or similar to compute minimal edits, converted to Yjs text operations

### 4. Sync Topology (Hybrid P2P + Relay)

```
┌─────────────────────────────────────────────────────────────┐
│                    Connection Strategy                        │
│                                                               │
│  On document open:                                            │
│       │                                                       │
│       ▼                                                       │
│  1. Connect to relay server (y-websocket) — ALWAYS            │
│     - Relay is the persistent CRDT store, auth gateway,       │
│       and awareness aggregator. It is NOT optional.           │
│       │                                                       │
│       ▼                                                       │
│  2. Also try WebRTC (y-webrtc) for direct peer connections    │
│     - Uses signaling server for initial handshake             │
│     - If peers are on same LAN, use mDNS for zero-config     │
│     - WebRTC is an optimization for lower latency, not        │
│       a primary transport                                     │
│       │                                                       │
│       ▼                                                       │
│  3. Relay server responsibilities:                            │
│     - Persistent CRDT state store (offline catch-up)          │
│     - Awareness aggregator (presence for all peers)           │
│     - Auth gateway (validates workspace access)               │
│     - Single source of truth when peers disagree              │
│                                                               │
│  Optimization:                                                │
│  - LAN peers discovered via mDNS → direct TCP, lowest        │
│    latency                                                    │
│  - Internet peers → WebRTC when possible for lower latency    │
│  - Relay ALWAYS receives updates (required for persistence)   │
│                                                               │
└─────────────────────────────────────────────────────────────┘
```

### 5. Daemon Architecture (Hybrid Embedded / Standalone)

The Scriptum daemon is a Rust process that owns all local collaboration state. It uses a hybrid architecture: it can run embedded in-process or as a standalone background process.

```
┌─────────────────────────────────────────────────────────────┐
│                    Daemon Deployment Modes                    │
│                                                               │
│  Mode 1: Embedded (Desktop App running)                       │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  Tauri App Process                                      │ │
│  │  ┌───────────────────────────────────────────────────┐  │ │
│  │  │  Daemon (in-process, linked as Rust library)      │  │ │
│  │  │  - Yjs docs, file watcher, section awareness      │  │ │
│  │  │  - Agent state, git sync scheduling               │  │ │
│  │  │  - Listens on Unix socket / named pipe for CLI    │  │ │
│  │  └───────────────────────────────────────────────────┘  │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                               │
│  Mode 2: Standalone (CLI / MCP without Desktop App)           │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  Daemon Process (background, auto-started on first use) │ │
│  │  - Same code as embedded, running as standalone binary  │ │
│  │  - Listens on Unix socket / named pipe                  │ │
│  │  - Started by: `scriptum` CLI or MCP server on first    │ │
│  │    connection attempt                                   │ │
│  │  - Stays running until explicit stop or machine reboot  │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                               │
│  Single daemon per machine, shared by all clients.            │
│  If desktop app starts and standalone daemon is running,      │
│  desktop takes over (standalone exits gracefully).            │
│                                                               │
│  Daemon owns:                                                 │
│  - Yjs document state (in-memory + persisted to .yjs files)  │
│  - File watcher (fsevents/inotify, runs in Rust)             │
│  - Section awareness layer                                    │
│  - Agent state persistence                                    │
│  - Git sync scheduling                                        │
│                                                               │
│  IPC:                                                         │
│  - Unix socket (macOS/Linux): ~/.scriptum/daemon.sock         │
│  - Named pipe (Windows): \\.\pipe\scriptum-daemon             │
│  - Protocol: JSON-RPC over the socket                         │
│                                                               │
│  Crash recovery:                                              │
│  - Yjs state persisted to .yjs files on every update          │
│  - On restart, daemon reloads .yjs files and resumes          │
│  - Clients reconnect automatically via socket                 │
│                                                               │
└─────────────────────────────────────────────────────────────┘
```

### 6. Git Sync Pipeline

```
┌─────────────────────────────────────────────────────────────┐
│                    Git Sync Pipeline                          │
│                                                               │
│  Trigger: User saves OR 30s idle timeout                      │
│       │                                                       │
│       ▼                                                       │
│  Collect all changed files since last commit                  │
│       │                                                       │
│       ▼                                                       │
│  Generate diff summary                                        │
│       │                                                       │
│       ▼                                                       │
│  Call Claude API with diff + context:                         │
│  "Generate a concise git commit message for these changes:    │
│   - Modified: docs/auth.md (rewrote OAuth section)            │
│   - Added: docs/api-keys.md (new document)                    │
│   - Modified: README.md (updated links)"                      │
│       │                                                       │
│       ▼                                                       │
│  Claude returns: "Add API key docs and update OAuth flow      │
│                   with PKCE support"                           │
│       │                                                       │
│       ▼                                                       │
│  Create git commit with:                                      │
│  - AI-generated message                                       │
│  - Co-authored-by headers for all editors since last commit   │
│  - Scriptum metadata in commit trailer                        │
│       │                                                       │
│       ▼                                                       │
│  Push to configured remote (if auto-push enabled)             │
│                                                               │
│  Example commit:                                              │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ Add API key docs and update OAuth flow with PKCE        │ │
│  │                                                          │ │
│  │ Co-authored-by: Gary <gary@example.com>                  │ │
│  │ Co-authored-by: Claude <agent:claude-1@scriptum>         │ │
│  │ Scriptum-Session: abc123                                 │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                               │
└─────────────────────────────────────────────────────────────┘
```

### 7. Agent Architecture

*(Directly informed by [Niwa](https://github.com/secemp9/niwa)'s agent-first design patterns)*

```
┌─────────────────────────────────────────────────────────────┐
│                    Agent Integration Layers                   │
│                                                               │
│  Layer 1: MCP Server (highest fidelity)                       │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  Tools:                                                  │ │
│  │  - scriptum_read(doc, section?) → content + metadata     │ │
│  │  - scriptum_edit(doc, section, content, summary)         │ │
│  │  - scriptum_list() → workspace documents                 │ │
│  │  - scriptum_tree(doc) → section structure with IDs       │ │
│  │  - scriptum_status() → agent's active sections/overlaps  │ │
│  │  - scriptum_conflicts() → section overlap warnings       │ │
│  │  - scriptum_history(doc) → version timeline              │ │
│  │  - scriptum_subscribe(doc) → change notifications        │ │
│  │  - scriptum_agents() → list active agents in workspace   │ │
│  │                                                          │ │
│  │  Resources:                                              │ │
│  │  - scriptum://docs/{id} → document content               │ │
│  │  - scriptum://docs/{id}/sections → section tree          │ │
│  │  - scriptum://workspace → workspace listing              │ │
│  │  - scriptum://agents → active agent list                 │ │
│  │                                                          │ │
│  │  All operations go through CRDT → full real-time sync    │ │
│  │  Attribution: agent name from MCP client config           │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                               │
│  Layer 2: CLI (good fidelity)                                 │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  $ scriptum edit doc.md \                                │ │
│  │      --section "## Auth" \                               │ │
│  │      --content "new content" \                           │ │
│  │      --agent claude-1 \                                  │ │
│  │      --summary "Updated OAuth flow"                      │ │
│  │                                                          │ │
│  │  Agent state commands (inspired by Niwa):                │ │
│  │  $ scriptum whoami          # Suggest unique name        │ │
│  │  $ scriptum status --agent claude-1                      │ │
│  │  $ scriptum conflicts --agent claude-1                   │ │
│  │  $ scriptum agents          # List all active agents     │ │
│  │  $ scriptum tree doc.md     # Section IDs for targeting  │ │
│  │                                                          │ │
│  │  CLI connects to local Scriptum daemon via Unix socket   │ │
│  │  Operations go through CRDT → full real-time sync        │ │
│  │  Attribution: explicit --agent flag                       │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                               │
│  Layer 3: File System Watching (fallback)                     │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  Agent writes to ~/.scriptum/workspaces/myproject/doc.md │ │
│  │                                                          │ │
│  │  File watcher detects change                             │ │
│  │  Diff computed → Yjs operations → synced to peers        │ │
│  │  Attribution: inferred from OS user/process when possible│ │
│  │  Lower fidelity: paragraph-level diff, no section target │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                               │
└─────────────────────────────────────────────────────────────┘
```

#### Agent State Management

Key Niwa insight: agents lose context (sub-agent spawns, context compaction, crashes). The daemon must persist agent state so they can recover.

```
┌─────────────────────────────────────────────────────────────┐
│              Agent State (persisted in daemon)                │
│                                                               │
│  Per agent (keyed by agent name):                             │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  agent_id: "claude-1"                                    │ │
│  │  first_seen: 2025-01-15T10:00:00Z                        │ │
│  │  last_seen: 2025-01-15T10:30:00Z                         │ │
│  │                                                          │ │
│  │  active_sections: [                                      │ │
│  │    { doc: "auth.md", section: "h2:oauth", since: ... }   │ │
│  │  ]                                                       │ │
│  │  # Sections this agent has read (registered intent)       │ │
│  │  # Cleared when agent edits the section or times out      │ │
│  │                                                          │ │
│  │  recent_edits: [                                         │ │
│  │    { doc: "auth.md", section: "h2:oauth",                │ │
│  │      summary: "Added PKCE flow", at: ... }               │ │
│  │  ]                                                       │ │
│  │  # Last N edits by this agent (for status recovery)       │ │
│  │                                                          │ │
│  │  overlaps: [                                             │ │
│  │    { doc: "auth.md", section: "h2:oauth",                │ │
│  │      other_agent: "claude-2", since: ... }               │ │
│  │  ]                                                       │ │
│  │  # Active section overlaps with other agents              │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                               │
│  This state survives:                                         │
│  - Agent context switches / sub-agent spawns                  │
│  - Context compaction (/compact in Claude Code)               │
│  - Agent crashes / restarts                                   │
│  - Daemon restarts (persisted to disk)                        │
│                                                               │
│  Recovery flow (what a fresh agent does):                     │
│  1. scriptum whoami → get suggested name + workspace summary  │
│  2. scriptum status --agent <name> → recover full state       │
│  3. scriptum conflicts --agent <name> → see any overlaps      │
│  4. Resume editing with full context                          │
│                                                               │
└─────────────────────────────────────────────────────────────┘
```

#### Claude Code Hooks

*(Directly ported from Niwa's hook system, adapted for Scriptum's CRDT architecture)*

```
┌─────────────────────────────────────────────────────────────┐
│              Claude Code Hook Integration                     │
│                                                               │
│  Setup: scriptum setup claude                                 │
│  Creates: .claude/settings.json with hook config              │
│                                                               │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ SessionStart                                             │ │
│  │ Trigger: Claude Code session begins                      │ │
│  │ Action:  Inject into context:                            │ │
│  │   - Scriptum CLI quick reference                         │ │
│  │   - Current workspace status (docs, active agents)       │ │
│  │   - This agent's state (if resuming)                     │ │
│  │   - Section overlap warnings (if any)                    │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                               │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ PreCompact                                               │ │
│  │ Trigger: Before /compact command                         │ │
│  │ Action:  Preserve Scriptum context for post-compaction:  │ │
│  │   - CLI reference (so Claude remembers commands)         │ │
│  │   - Agent name and current state                         │ │
│  │   - Active sections and any overlaps                     │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                               │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ PreToolUse (matcher: Write|Edit on *.md files)           │ │
│  │ Trigger: Claude is about to edit a .md file directly     │ │
│  │ Action:  Provide context:                                │ │
│  │   - "Consider using `scriptum edit` for better           │ │
│  │     attribution and section-level sync"                  │ │
│  │   - Warn if another agent is editing same section        │ │
│  │   - Show current section state                           │ │
│  │ Note: Does NOT block the edit - just provides context    │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                               │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ PostToolUse (matcher: Write|Edit on *.md files)          │ │
│  │ Trigger: Claude just edited a .md file directly          │ │
│  │ Action:  Confirm sync:                                   │ │
│  │   - "File watcher detected change, synced to CRDT"       │ │
│  │   - Show if any section overlaps resulted from the edit  │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                               │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ Stop                                                     │ │
│  │ Trigger: Claude Code session ending                      │ │
│  │ Action:  Reminder:                                       │ │
│  │   - Any unsynced local changes                           │ │
│  │   - Any active section overlaps to be aware of           │ │
│  │   - "Your agent state is preserved for next session"     │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                               │
└─────────────────────────────────────────────────────────────┘
```

---

## Data Model

### Local Storage (per device)

```
~/.scriptum/
├── config.toml                  # Global config (API keys, defaults)
├── workspaces/
│   ├── my-project/
│   │   ├── .scriptum/
│   │   │   ├── workspace.toml   # Workspace config (git remote, sync settings)
│   │   │   ├── crdt/            # Yjs binary state for each document
│   │   │   │   ├── doc1.yjs
│   │   │   │   └── doc2.yjs
│   │   │   ├── meta.db          # SQLite: document metadata, tags, links
│   │   │   └── git/             # Git repo (if git sync enabled)
│   │   ├── README.md            # Actual markdown files
│   │   ├── docs/
│   │   │   ├── auth.md
│   │   │   └── api.md
│   │   └── ...
│   └── another-workspace/
│       └── ...
└── daemon.sock                  # Unix socket for CLI ↔ daemon communication
```

### Relay Server Storage

```
/data/scriptum/
├── relay.db                     # SQLite: all relay metadata (single database)
│                                #   - workspaces (name, members, settings)
│                                #   - users (accounts, OAuth tokens)
│                                #   - invites (workspace invitations)
│                                #   - document metadata
├── crdt/
│   ├── {workspace_id}/
│   │   ├── {doc_id}.yjs         # Yjs binary state (append-only updates)
│   │   └── ...
│   └── ...
└── (awareness is ephemeral, in-memory only)
```

### Document Metadata Schema

```sql
-- Stored in local SQLite (meta.db)
CREATE TABLE documents (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL,           -- Relative path in workspace (e.g., "docs/auth.md")
    title TEXT,
    created_at DATETIME,
    updated_at DATETIME,
    last_edited_by TEXT,          -- Agent/user name
    word_count INTEGER,
    git_synced_at DATETIME,      -- Last git commit timestamp
    git_commit_hash TEXT          -- Last git commit hash
);

CREATE TABLE tags (
    doc_id TEXT REFERENCES documents(id),
    tag TEXT NOT NULL
);

CREATE TABLE backlinks (
    source_doc_id TEXT REFERENCES documents(id),
    target_doc_id TEXT REFERENCES documents(id),
    link_text TEXT               -- The [[link text]] used
);

CREATE TABLE edit_sessions (
    id TEXT PRIMARY KEY,
    doc_id TEXT REFERENCES documents(id),
    editor_name TEXT,            -- Human name or agent ID
    editor_type TEXT,            -- 'human', 'agent', 'unknown'
    started_at DATETIME,
    ended_at DATETIME,
    sections_edited TEXT,        -- JSON array of section IDs
    summary TEXT                 -- AI-generated or user-provided
);
```

---

## Key Algorithms

### Markdown ↔ CRDT Round-Trip

Since we use Y.Text storing raw markdown, the round-trip is trivially an identity mapping. There is no format normalization problem.

```
CRDT model: Y.Text stores the raw markdown string exactly as typed.
No normalization, no conversion between rich document models.

ytext.toString() === file.md contents  (identity mapping)

This means:
1. File on disk = ytext.toString() = raw markdown (byte-for-byte)
2. Editor (CodeMirror 6) renders live preview via custom extension:
   - Active line shows raw markdown (for editing)
   - Unfocused lines render as rich text (for reading)
   - The live preview NEVER alters the stored text
3. Edits in the editor produce CodeMirror transactions → y-codemirror.next
   converts them to Yjs text ops
4. File watcher edits produce text diffs → converted to Yjs text ops
5. Both pathways produce Yjs operations → CRDT merges them

The key insight: Yjs is the source of truth, not the file. The file is a
projection of the Yjs state. Because Y.Text stores raw markdown, there
is no lossy conversion — exact formatting is preserved, including
whitespace, indentation style, and line endings.

Reference: codemirror-markdown-hybrid (MIT) for the hybrid rendering approach.
```

### Diff-to-Yjs Conversion (for file watcher)

```typescript
function applyFileChangeToYjs(oldContent: string, newContent: string, ytext: Y.Text) {
  // Use Myers diff or similar to get minimal edit operations
  const patches = diffMatchPatch.patch_make(oldContent, newContent)

  // Convert patches to Yjs operations
  ydoc.transact(() => {
    let offset = 0
    for (const patch of patches) {
      const start = patch.start1 + offset

      for (const [op, text] of patch.diffs) {
        if (op === DIFF_DELETE) {
          ytext.delete(start, text.length)
          offset -= text.length
        } else if (op === DIFF_INSERT) {
          ytext.insert(start, text)
          offset += text.length
        }
        // DIFF_EQUAL: advance position
      }
    }
  }, 'file-watcher')  // Origin tag for identifying source of change
}
```

### AI Commit Message Generation

```typescript
async function generateCommitMessage(changes: FileChange[]): Promise<string> {
  const diffSummary = changes.map(c => {
    const stats = `+${c.additions}/-${c.deletions}`
    return `${c.status} ${c.path} (${stats}): ${c.sectionsSummary}`
  }).join('\n')

  const response = await claude.messages.create({
    model: 'claude-haiku-4-5-20250929',
    max_tokens: 200,
    system: `Generate a concise git commit message (max 72 chars for first line).
             Focus on WHAT changed and WHY, not HOW. Use imperative mood.
             If multiple files changed, summarize the overall intent.`,
    messages: [{
      role: 'user',
      content: `Generate a commit message for these changes:\n\n${diffSummary}`
    }]
  })

  return response.content[0].text
}
```

---

## Security Considerations

- **CRDT encryption**: Yjs updates encrypted at rest and in transit (TLS for WebSocket, libsodium for stored state)
- **Auth tokens**: Short-lived JWTs for relay server access, refreshed via OAuth
- **Workspace isolation**: Relay server enforces workspace boundaries - no cross-workspace data access
- **Local data**: CRDT state files are only readable by the owning user (0600 permissions)
- **Git credentials**: Stored in OS keychain (macOS Keychain, Linux Secret Service), never in config files
- **AI API keys**: Stored in OS keychain, used only for commit message generation
- **Audit log**: All relay server access logged with IP, user, and workspace

---

## Performance Targets

| Metric | Target | Approach |
|--------|--------|----------|
| Editor keystroke latency | < 16ms | Local-first CRDT, async sync |
| CRDT sync latency (P2P) | < 100ms | WebRTC data channels |
| CRDT sync latency (relay) | < 300ms | WebSocket, binary encoding |
| File watcher response | < 200ms | fsevents (macOS) / inotify (Linux) |
| Git commit generation | < 2s | Claude Haiku for commit messages (fast, cheap) |
| Desktop app startup | < 1s | Tauri, lazy document loading |
| Desktop app memory | < 100MB | Rust backend, efficient CRDT encoding |
| Web app initial load | < 2s | Code splitting, edge CDN |

---

## Development Phases

### Phase 1: Foundation (Weeks 1-4)
- [ ] Tauri app scaffold with React + CodeMirror 6 editor
- [ ] Build custom Live Preview CM6 extension (hybrid rendering: active line raw markdown, unfocused lines rich text). Reference: codemirror-markdown-hybrid (MIT)
- [ ] Yjs integration with CodeMirror 6 via y-codemirror.next (local CRDT, no network)
- [ ] File watcher daemon (bidirectional sync: editor ↔ local files)
- [ ] Basic workspace management (create, open, list documents)
- [ ] Markdown rendering + editing (GFM support)

### Phase 2: Collaboration (Weeks 5-8)
- [ ] WebSocket relay server (Rust/Axum)
- [ ] y-websocket provider for document sync
- [ ] WebRTC provider (y-webrtc) for P2P optimization (relay is primary)
- [ ] Presence/awareness (live cursors, online indicators)
- [ ] Section awareness layer
- [ ] Basic web app (shared React components with desktop)

### Phase 3: Git & AI (Weeks 9-12)
- [ ] Git sync engine (gitoxide or libgit2)
- [ ] AI commit message generation (Claude API)
- [ ] Auto-commit on save/idle
- [ ] Git history browsing in the UI
- [ ] Attribution tracking (per-edit, per-section)

### Phase 4: Agent Integration (Weeks 13-16)
- [ ] `scriptum` CLI tool (Rust) - connects to daemon via Unix socket
- [ ] CLI commands: `read`, `edit`, `tree`, `sections`, `search`, `diff`, `ls`
- [ ] Agent state commands: `whoami`, `status`, `conflicts`, `agents` (inspired by Niwa)
- [ ] Agent state persistence in daemon (survives context switches)
- [ ] MCP server for Claude Code / Cursor (TypeScript, stdio transport)
- [ ] MCP tools: `scriptum_read`, `scriptum_edit`, `scriptum_tree`, `scriptum_status`, `scriptum_conflicts`
- [ ] Claude Code hooks: SessionStart, PreCompact, PreToolUse, PostToolUse, Stop (ported from Niwa)
- [ ] `scriptum setup claude` command to install hooks
- [ ] Agent attribution in UI (name badges, contribution indicators)

### Phase 5: Polish & Launch (Weeks 17-20)
- [ ] Permissions & sharing
- [ ] Search (full-text across documents)
- [ ] Tags, backlinks, wiki-style linking
- [ ] Commenting / inline threads
- [ ] Version history timeline UI
- [ ] Documentation & onboarding
- [ ] Performance optimization & testing

---

## Open Questions

1. ~~**Tiptap licensing**~~: **RESOLVED** — Using CodeMirror 6 (MIT licensed). No licensing issue.

2. ~~**Markdown fidelity**~~: **RESOLVED** — Preserve exact formatting. Y.Text stores raw markdown as-is, no normalization.

3. **Large documents**: Yjs performance degrades with very large documents (>1MB). Strategy: document size limit? Auto-splitting into sections? Lazy loading of CRDT state?

4. **Mobile**: No mobile app in V1. When we add it, should it be React Native (share code) or native (better UX)?

5. **Pricing model**: Open-source relay server + hosted option. What's the hosted pricing? Per-user? Per-workspace? Free tier?

6. **CRDT garbage collection**: Yjs accumulates tombstones. Strategy for periodic garbage collection without breaking sync with offline peers?

7. **Daemon IPC protocol**: JSON-RPC over Unix socket (macOS/Linux) / named pipe (Windows) between daemon and MCP server / CLI. Exact message schema and error handling conventions TBD.
