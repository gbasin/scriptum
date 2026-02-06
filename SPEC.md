# Scriptum - Complete Specification

> Local-first collaborative markdown with seamless git sync and first-class agent support.

---

# Part 1: Product

## Vision

Writing should feel like writing, not committing. Collaboration should be effortless. Your files are yours, locally.

Scriptum bridges the gap between GitHub (too heavy for collaboration) and Notion (too locked-in, hostile to local editing). It's a **hosted markdown collaboration tool** that lets you edit files locally with any editor, collaborate in real-time on the web, and automatically syncs to git with AI-generated commit messages.

## Core Principles

1. **Local-first**: Your data lives on your machine. Local replica is authoritative — solo and offline use work fully without a server. The relay is required only for multi-user collaboration and cross-device sync.
2. **Markdown-native**: Pure `.md` files on disk. No proprietary format. No lock-in.
3. **Conflict-free data, coordinated editing**: CRDT ensures no data loss at the data layer — every character from every editor is preserved. Intent system and section awareness ensure no surprises at the UX layer — concurrent edits to the same section are surfaced for review, not silently interleaved.
4. **Git-optional**: Push to GitHub/any git remote with AI-generated commits. Or don't. Git is a sync target, not a dependency.
5. **Agent-friendly**: AI agents are first-class collaborators with attribution, not second-class citizens bolted on.

---

## Target Users

### Primary
- **Developer teams**: Technical docs, RFCs, ADRs, READMEs, runbooks
- **Mixed teams**: Engineers + PMs + designers collaborating on specs, proposals, planning docs
- **AI-assisted workflows**: Claude Code, Cursor, custom agents editing docs alongside humans

### Secondary
- **Personal knowledge base**: Notes, journals, research - personal Obsidian alternative with collaboration
- **Open source projects**: Documentation that multiple contributors can edit simultaneously

---

## Features

### 1. Rich Markdown Editor

A hybrid live-preview editing experience powered by **CodeMirror 6** with a custom live preview extension (reference: `codemirror-markdown-hybrid`, MIT). The active line shows raw markdown for precise editing; unfocused lines render as rich text (similar to Obsidian's live preview). This preserves exact markdown formatting -- what you type is what gets stored.

- **Hybrid live preview**: Active line shows raw markdown, unfocused lines render inline as rich text (headings, bold, links, etc.)
- **Exact formatting preservation**: No normalization. Raw markdown is stored as-is in the CRDT and on disk.
- **Pure markdown storage**: Files on disk are always valid `.md` files -- the exact bytes you typed
- **GFM core + extensions**: Tables, task lists, footnotes, code blocks with syntax highlighting. Extensions beyond GFM: math (KaTeX), diagrams (Mermaid).
- **Slash commands**: `/table`, `/code`, `/image`, `/callout` for quick insertion
- **Drag and drop**: Images, files - auto-uploaded and linked

### 2. Real-Time Collaboration

Multiple people editing the same document simultaneously.

- **Live cursors**: See where collaborators are typing, with name labels
- **Presence indicators**: Who's online, who's viewing which document
- **Character-level CRDT**: Powered by Yjs - merges are conflict-free at the character level
- **Section awareness**: Overlay on top of CRDT that detects when two editors are in the same markdown section, showing a subtle indicator
- **Offline support**: Edit offline, changes merge seamlessly when you reconnect
- **Intent/lease system**: Editors can "claim" a section with a TTL (e.g., `scriptum_claim(section, ttl=10m, note="rewriting auth")`). UI shows "claimed by X" as a soft advisory lock. Other editors see a warning but can still edit — leases are non-blocking. Provides a default coordination path for agents and humans.
- **Reconciliation UI**: When concurrent large rewrites hit the same section, both versions are preserved and shown side-by-side for the user to choose, merge, or keep both. Prevents the worst CRDT interleaving UX.
- **Commenting**: Inline comments on selections, threaded discussions, resolve/unresolve. Comments are for discussion and conversation -- not an approval workflow. No tracked changes or suggesting mode.

### 3. Local Editing & Sync

Edit with any tool on your machine - VS Code, Vim, Claude Code, custom scripts.

- **Local file sync**: Documents exist as `.md` files in a local folder
- **File watcher**: Background daemon detects local file changes, diffs them, and feeds changes into the CRDT
- **Bidirectional sync**: Changes from remote collaborators are written back to local files
- **Any editor**: If it can edit a file, it works with Scriptum
- **Conflict-free**: Even rapid local edits from multiple tools merge cleanly via CRDT

### 4. Agent Integration (First-Class)

AI agents are treated as collaborators, not tools.

**CLI (`scriptum` command)** *(inspired by [Niwa](https://github.com/secemp9/niwa)'s agent-first CLI design)*:
```bash
# Core editing workflow
scriptum read doc.md --section "## Authentication" --agent claude-1
scriptum edit doc.md --section "## Authentication" --content "new content" --agent claude-1 --summary "Added OAuth2 PKCE flow"
scriptum edit doc.md --section "## Authentication" --file content.md --agent claude-1  # From file (for complex content)

# Agent state management (critical for sub-agents / context switches)
scriptum whoami                      # Suggest unique agent name, show workspace state
scriptum status --agent claude-1     # Pending edits, active sections, recent changes by this agent
scriptum conflicts --agent claude-1  # Section-level overlap warnings (other agents editing same sections)
scriptum agents                      # List all agents currently interacting with this workspace

# Workspace operations
scriptum ls                          # List workspace documents
scriptum tree doc.md                 # Show document section structure with IDs
scriptum diff doc.md                 # Show pending changes since last git commit
scriptum search "authentication"     # Full-text search across workspace

# Section targeting
scriptum sections doc.md             # List all sections with IDs, versions, last editor
scriptum peek doc.md --section "## Auth"  # Quick read without registering edit intent
```

**Key CLI design principles** (learned from Niwa):
- **Explicit `--agent` flag**: Every mutating operation requires an agent name for attribution
- **`--summary` on edits**: Agents describe their intent, which helps other agents understand concurrent changes
- **`read` before `edit`**: Reading a section registers intent, enabling smarter overlap detection
- **State survives context switches**: Agent state (pending reads, active sections) persists in the daemon, so sub-agents spawned with fresh context can run `scriptum status` to recover state
- **`--file` flag for complex content**: Avoids shell quoting issues with multi-line markdown

**MCP Server**:
- Native integration with Claude Code, Cursor, and any MCP-compatible agent
- Tools: `scriptum_read`, `scriptum_edit`, `scriptum_list`, `scriptum_tree`, `scriptum_status`, `scriptum_conflicts`, `scriptum_history`, `scriptum_subscribe`, `scriptum_agents`, `scriptum_claim`, `scriptum_bundle`
- `scriptum_claim(section_id, ttl, mode, note)`: Claim a section with advisory lease
- `scriptum_bundle(doc, section_id, include=[parents, children, backlinks, comments], token_budget=N)`: Get optimally-sized context for a section in one call — agents get just enough context, continuously updated
- Resources: `scriptum://docs/{id}`, `scriptum://docs/{id}/sections`, `scriptum://workspace`, `scriptum://agents`
- Agents receive notifications when sections they're working on change
- Agent name derived from MCP client config (no manual `--agent` needed)

**Claude Code Hooks** *(directly inspired by Niwa)*:
```bash
scriptum setup claude      # Install hooks into .claude/settings.json
scriptum setup claude --remove
```

| Hook | Trigger | Action |
|------|---------|--------|
| **SessionStart** | Claude session begins | Injects workspace status + CLI quick reference into context |
| **PreCompact** | Before `/compact` | Preserves Scriptum context so Claude remembers docs state after compaction |
| **PreToolUse** | Before Write/Edit on `.md` files | Warns if other agents are editing the same section, suggests using `scriptum edit` instead |
| **PostToolUse** | After Write/Edit on `.md` files | Confirms file watcher picked up the change and synced to CRDT |
| **Stop** | Session ending | Reminds about any unsynced changes or section overlaps |

**File System Watching** (fallback):
- Agents that can't use CLI/MCP can just edit `.md` files directly
- File watcher picks up changes, diffs, and merges via CRDT
- Attribution inferred from process/user context when possible
- Lower fidelity than CLI/MCP: paragraph-level diff, no section targeting, no edit summaries

**Agent Attribution** (dual persistence model):
- **CRDT-level**: Lightweight origin tag embedded in each Yjs transaction (author ID + timestamp, ~20 bytes per update). Travels with the CRDT, survives storage and replay. Enables offline attribution.
- **Server-level**: Relay annotates each update with authenticated user/agent ID in `yjs_update_log`. Provides authoritative mapping for history and audit.
- Both layers combined enable: per-section "last edited by" in UI, `scriptum blame` command with full edit summaries, character-level time travel with authorship.
- Contributions flow through to git via `Co-authored-by` trailers (best-effort attribution at git layer).

**Agent Name Policy**:
- Duplicate agent names are allowed. Two agents with the same name share state — their edits are attributed to the same identity.
- This simplifies agent lifecycle: agents are "disposable," and multiple instances of the same agent type (e.g., `claude`) naturally share context.
- For distinct tracking, agents should use unique names (e.g., `claude-auth-reviewer`, `claude-api-writer`).

### 5. Git Sync

Automatic, intelligent syncing to git remotes.

- **Semantic commit groups**: Commits triggered by intent closure (finishing a claimed section, resolving a comment thread, completing a task, explicit checkpoint command). Idle timer (~30 seconds) as fallback only — not the primary commit trigger.
- **AI-generated commit messages**: Analyzes the diff and produces meaningful commit messages (e.g., "Update authentication flow with OAuth2 PKCE details")
- **Configurable remote**: GitHub, GitLab, Bitbucket, any git remote
- **Branch strategy**: Configurable per workspace - commit to main, to a branch, or create PRs
- **Selective sync**: Choose which documents/folders sync to git
- **Two-tier attribution**: `scriptum blame` shows per-line/per-section attribution from CRDT data (richer than git — includes edit summaries, agent vs human, section context). Git history preserves co-authors via `Co-authored-by` trailers as best-effort.

### 6. Workspace Organization

Abstract workspace layer that's flexible and intuitive.

- **Workspaces**: Top-level containers for related documents
- **Folders**: Hierarchical organization within workspaces
- **Tags**: Cross-cutting labels for documents (e.g., `#rfc`, `#draft`, `#approved`)
- **Backlinks**: `[[wiki-style]]` links between documents for navigation convenience. This is a linking/navigation feature, not a wiki system -- no namespaces, templates, or wiki-specific features.
- **Search**: Full-text search across all documents, with filters by tag, author, date
- **Flexible backends**: A workspace can be backed by:
  - A git repo (full sync)
  - A local folder (file system only)
  - Local files are always present -- the relay server is for sync and collaboration, not primary storage

### 7. Version History & Attribution

Full audit trail of every change.

- **CRDT history (recent, V1)**: Character-level, real-time history for the last 90 days. See exactly who typed what, rewind to any point. Requires: update log with timestamps, stable client-ID-to-user mapping (via dual attribution model), periodic snapshots for feasible replay, scrub/replay UI.
- **Git history (permanent)**: Every auto-commit is a permanent snapshot. Browse with familiar git tools.
- **Timeline view**: Visual timeline of document evolution with contributor avatars
- **Diff view**: Side-by-side or inline diffs between any two versions
- **Restore**: One-click restore to any previous version
- **Per-editor breakdown**: See how much each collaborator (human or agent) contributed

### 8. Permissions & Sharing

Control who can access and edit.

- **Workspace-level permissions**: Owner, Editor, Viewer roles
- **Document-level overrides**: Lock specific docs or grant access to specific people
- **Share links**: Public or password-protected links for external sharing
- **Guest access**: Invite non-users to view/edit specific documents without creating an account

---

## User Flows

### Flow 1: Solo Writer with Git Sync

1. Install Scriptum desktop app
2. Create a workspace backed by a GitHub repo
3. Write in the rich editor or in VS Code via local files
4. Changes auto-commit to GitHub after ~30 seconds of inactivity with AI-generated messages
5. View history in Scriptum or on GitHub

### Flow 2: Team Collaboration

1. Team lead creates a workspace, invites team
2. Team members install desktop app or use web UI
3. Multiple people edit the same RFC simultaneously
4. Live cursors show who's where, CRDT merges everything
5. Comments and threads for async discussion
6. Changes sync to shared GitHub repo

### Flow 3: AI-Assisted Writing

1. Open a doc in Scriptum desktop app
2. In another terminal, Claude Code edits the same doc via MCP server
3. Claude's changes appear in real-time in the editor
4. Human reviews, comments, or edits alongside Claude
5. All contributions attributed - "Section written by Claude, reviewed by Gary"
6. Auto-committed to git with clear attribution

### Flow 4: Multi-Agent Collaboration

1. Create a spec document in Scriptum
2. Assign different sections to different agents via CLI
3. Agents edit their sections simultaneously via `scriptum edit`
4. Section-level awareness alerts if two agents touch the same section
5. CRDT auto-merges non-overlapping changes
6. Human reviews the assembled document in the web/desktop UI

---

## Non-Goals (V1)

- **Not a wiki**: `[[backlinks]]` are a navigation convenience, not a wiki system. No namespaces, templates, transclusion, or wiki-specific features.
- **Not a CMS**: No publishing pipeline, SEO, or public-facing rendering. No CMS publishing workflows.
- **Not Notion**: No databases, kanban boards, or non-document content types.
- **Not Google Docs**: No suggesting mode, tracked changes, or approval workflows. Comments exist for discussion, not as an approval primitive.
- **No native mobile clients** in V1.
- **No arbitrary binary asset collaboration** in V1.

---

## Success Metrics

- **Time to first collaborative edit**: < 2 minutes from install
- **Sync latency**: < 500ms for CRDT updates between peers
- **Git commit quality**: AI commit messages rated "good" by users >80% of the time
- **Offline resilience**: 100% of offline edits merge without data loss
- **Agent integration**: Claude Code can edit a Scriptum doc with < 5 lines of config

---

# Part 2: Technical Architecture

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
                            │  │  WebRTC    │  │  ← P2P (optimization)
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

Core architectural decisions:
1. Canonical state is CRDT; markdown is a projection.
2. **Yjs-in-daemon model**: The daemon owns the authoritative Y.Doc and exposes a local WebSocket server (`ws://localhost:{port}`). Desktop/web CodeMirror connects via y-websocket as a standard Yjs provider. CLI and MCP connect as peers over the same local WS. On mobile, the daemon is embedded in-process (same as Tauri desktop mode).
3. Relay assigns monotonic `server_seq` per `(workspace_id, doc_id)`.
4. Local writes are acknowledged only after durable local WAL fsync.
5. Protocol compatibility target is N and N-1 for REST, WS, JSON-RPC.
6. **Web-first onboarding**: Web app is a first-class client. Web users can create workspaces, edit, and collaborate without installing anything (CRDT state on relay). Desktop adds local files, offline mode, CLI, and agent integration. "Local files always present" applies to desktop users only.

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
| Storage | **PostgreSQL 15+** | All relay metadata: workspaces, users, documents, update index |
| CRDT Storage | **Object storage** | Yjs binary snapshots for large documents |

### Git Sync
| Component | Technology | Rationale |
|-----------|-----------|-----------|
| Git ops | **gitoxide** (Rust) or **libgit2** | Fast, no git CLI dependency |
| AI commits | **Claude API** | Generate commit messages from diffs |
| Scheduling | **Built into daemon** | Timer-based, triggers on save/idle |

### Build & Dev Tooling

**Monorepo Structure**:

| Layer | Tool | Rationale |
|-------|------|-----------|
| Orchestration | **Turborepo** | Task caching, dependency-aware builds, parallel execution across TS packages |
| Rust workspace | **Cargo workspace** | Native Rust multi-crate management (daemon, CLI, relay share code) |
| TS package manager | **pnpm** | Fast, strict, disk-efficient. Turborepo's recommended pairing |

```
scriptum/
├── turbo.json                  # Turborepo pipeline config
├── Cargo.toml                  # Cargo workspace root
├── crates/
│   ├── daemon/                 # scriptumd (Yjs engine, file watcher, WS server)
│   ├── cli/                    # scriptum CLI
│   ├── relay/                  # Relay server (Axum)
│   └── common/                 # Shared Rust: CRDT ops, section parser, protocol types
├── packages/
│   ├── web/                    # Web app (React + CodeMirror 6)
│   ├── desktop/                # Tauri shell (wraps web + daemon)
│   ├── mcp-server/             # MCP server (TypeScript, stdio)
│   ├── editor/                 # Shared CM6 extensions (live preview, collaboration)
│   └── shared/                 # Shared TS types, API client, protocol definitions
└── .github/workflows/          # CI
```

**TypeScript Tooling**:

| Concern | Tool | Rationale |
|---------|------|-----------|
| Lint + Format | **Biome** | Single Rust-based tool, ~100x faster than ESLint+Prettier. Handles both linting and formatting with minimal config |
| Test runner | **Vitest** | Vite-native, fast HMR-aware watch mode, Jest-compatible API, native ESM. Already using Vite for build |
| Type checking | **tsc --noEmit** | Standard TypeScript compiler. Biome handles style, tsc handles types |
| Bundling | **Vite** | Already chosen for desktop/web. Handles dev server, HMR, production builds |

**Rust Tooling**:

| Concern | Tool | Rationale |
|---------|------|-----------|
| Lint | **clippy** | Standard Rust linter, catches common mistakes and non-idiomatic code |
| Format | **rustfmt** | Standard Rust formatter, zero-config |
| Test runner | **cargo-nextest** | Parallel test execution, up to 3x faster than `cargo test`. Better output, retries, JUnit XML for CI |
| Coverage | **cargo-llvm-cov** | Source-based coverage via LLVM instrumentation. Accurate, fast |
| Audit | **cargo-deny** | License checking, vulnerability scanning, duplicate crate detection |

**CI (GitHub Actions)**:

| Job | Trigger | Contents |
|-----|---------|----------|
| **Lint & Format** | Every push/PR | `biome check`, `cargo clippy`, `cargo fmt --check`, `tsc --noEmit` |
| **Test (TS)** | Every push/PR | `vitest run` across all TS packages (Turborepo cached) |
| **Test (Rust)** | Every push/PR | `cargo nextest run` across all crates |
| **Golden file tests** | Every push/PR | Diff-to-Yjs edge case suite |
| **Property tests** | Nightly | CRDT convergence + diff-to-Yjs randomized scenarios |
| **Integration** | Every PR | Daemon+watcher, relay+WS, git worker end-to-end |
| **Security** | Every PR | `cargo deny check`, dependency audit, SAST |
| **Build artifacts** | Release tags | Tauri desktop (macOS/Linux/Windows), CLI binaries, Docker relay image |

**Release & Versioning**:

| Concern | Tool | Rationale |
|---------|------|-----------|
| Changelogs | **changesets** | Per-package versioning, grouped changelogs, PR-based workflow |
| Desktop distribution | **Tauri updater** | Built-in auto-update with user consent |
| Relay deployment | **Docker** | Containerized relay server, deployed via CI |

---

## System Architecture

### Availability Model

- **Local plane**: Always available. Offline edits succeed with crash-safe durability (WAL fsync before ack).
- **Collaboration plane**: Depends on relay for cross-device sync. Target >= 99.9% monthly availability.
- **Degraded mode**: When relay is unreachable, local edits queue in outbox. Sync resumes automatically on reconnect.

### Consistency Model

- Per-doc eventual consistency with deterministic convergence.
- Server ordering key: `(workspace_id, doc_id, server_seq BIGINT)`.
- Client dedupe key: `(client_id, client_update_id UUIDv7)`.
- At-least-once delivery; idempotent apply by dedupe key.

### Trust Boundaries

- Local daemon trusts only local OS user identity.
- Relay trusts bearer access token and workspace-scoped session token.
- Share links are capability tokens with bounded scope and expiry.

---

## Component Design

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

**Markdown / CRDT Round-Trip**:

```
CRDT model: Y.Text stores the raw markdown string exactly as typed.
No normalization, no conversion between rich document models.

ytext.toString() === file.md contents  (identity mapping)

This means:
1. File on disk = ytext.toString() = exact markdown content (no normalization, no reformatting; configurable line-ending policy, UTF-8)
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

**Daemon behavior**: Rebuilds heading tree after committed transaction. Stable `section_id = slug(ancestor_chain)+ordinal_suffix`. Emits awareness only, never blocks writes.

### 3. Daemon Architecture (Hybrid Embedded / Standalone)

The Scriptum daemon (`scriptumd`) is a Rust process that owns all local collaboration state. It uses a hybrid architecture: it can run embedded in-process or as a standalone background process. One daemon per OS user, exposing JSON-RPC over user-scoped Unix socket (macOS/Linux) or named pipe (Windows).

```
┌─────────────────────────────────────────────────────────────────┐
│                    Daemon Deployment Modes                        │
│                                                                   │
│  Mode 1: Embedded (Desktop App running)                           │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │  Tauri App Process                                          │ │
│  │  ┌───────────────────────────────────────────────────────┐  │ │
│  │  │  Daemon (in-process, linked as Rust library)          │  │ │
│  │  │  - Yjs docs, file watcher, section awareness          │  │ │
│  │  │  - Agent state, git sync scheduling                   │  │ │
│  │  │  - Listens on Unix socket / named pipe for CLI        │  │ │
│  │  └───────────────────────────────────────────────────────┘  │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                                                                   │
│  Mode 2: Standalone (CLI / MCP without Desktop App)               │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │  Daemon Process (background, auto-started on first use)     │ │
│  │  - Same code as embedded, running as standalone binary      │ │
│  │  - Listens on Unix socket / named pipe                      │ │
│  │  - Started by: `scriptum` CLI or MCP server on first       │ │
│  │    connection attempt                                       │ │
│  │  - Stays running until explicit stop or machine reboot      │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                                                                   │
│  Single daemon per machine, shared by all clients.                │
│  If desktop app starts and standalone daemon is running,          │
│  desktop takes over (standalone exits gracefully).                │
│                                                                   │
│  Daemon owns:                                                     │
│  - Yjs document state (in-memory + persisted to .yjs files)      │
│  - File watcher (fsevents/inotify, runs in Rust)                 │
│  - Section awareness layer                                        │
│  - Agent state persistence                                        │
│  - Git sync scheduling                                            │
│                                                                   │
│  IPC:                                                             │
│  - Unix socket (macOS/Linux): ~/.scriptum/daemon.sock             │
│  - Named pipe (Windows): \\.\pipe\scriptum-daemon                 │
│  - Protocol: JSON-RPC over the socket                             │
│                                                                   │
│  Crash recovery:                                                  │
│  - Load latest snapshot, replay WAL, mark doc degraded if         │
│    checksum fails                                                 │
│  - Clients reconnect automatically via socket                     │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

**Components**: file watcher, CRDT engine, section parser, git sync worker, local metadata DB (`meta.db` SQLite), outbound sync queue.

**Clients**: desktop app (Tauri+React+CM6), CLI (Rust), MCP server (TypeScript).

**Stores**: `crdt_store/` (append-only WAL + compressed snapshots), `meta.db` (SQLite).

**Durability**: Local write acknowledged only after WAL fsync.

**Outbox**: Exponential backoff 250ms..30s, max 8 immediate attempts, then deferred queue. Queue bounds: max 10,000 pending updates or 1 GiB per workspace, then `OUTBOX_BACKPRESSURE` alert.

### 4. File Watcher Pipeline

> **Ownership**: The file watcher runs inside the daemon (Rust). The TypeScript-style
> code examples below are illustrative pseudocode showing the algorithm, not the
> actual implementation language. The real implementation is in Rust.

```
┌─────────────────────────────────────────────────────────────────┐
│                    File Watcher Pipeline                          │
│                                                                   │
│  FS Event (create/modify/delete)                                  │
│       │                                                           │
│       ▼                                                           │
│  Debounce (100ms, configurable 50-500ms)                          │
│       │                                                           │
│       ▼                                                           │
│  Canonicalize path, reject traversal/symlink escape               │
│       │                                                           │
│       ▼                                                           │
│  Normalize path: separators to /, Unicode NFKC, no ./..,          │
│  max 512 chars                                                    │
│       │                                                           │
│       ▼                                                           │
│  Read file content + compute hash                                 │
│  (UTF-8/BOM handling, line-ending preservation)                   │
│       │                                                           │
│       ▼                                                           │
│  Compare hash with last known CRDT state                          │
│       │                                                           │
│  ┌────┴────┐                                                      │
│  │ Changed │                                                      │
│  └────┬────┘                                                      │
│       │                                                           │
│       ▼                                                           │
│  Compute diff (CRDT state → new file content)                     │
│       │                                                           │
│       ▼                                                           │
│  Apply diff as Yjs operations (origin-tag for loop prevention)    │
│       │                                                           │
│       ▼                                                           │
│  Yjs propagates to all connected peers                            │
│       │                                                           │
│       ▼                                                           │
│  (Do NOT write back to file - we just read from it)               │
│                                                                   │
│  ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─  │
│                                                                   │
│  Remote CRDT update received                                      │
│       │                                                           │
│       ▼                                                           │
│  Render Yjs state to markdown string                              │
│       │                                                           │
│       ▼                                                           │
│  Compare with current file on disk                                │
│       │                                                           │
│  ┌────┴────┐                                                      │
│  │ Changed │                                                      │
│  └────┬────┘                                                      │
│       │                                                           │
│       ▼                                                           │
│  Write to file (with watcher temporarily paused for this)         │
│                                                                   │
│  Disk write policy: ALWAYS write remote changes to disk.          │
│  External editors (VS Code, Vim) handle "file changed on disk"    │
│  prompts natively. This is the expected behavior — simple and     │
│  predictable. No shadow files or idle detection.                  │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

**Key details:**
- **Debouncing**: 100ms debounce on file events to batch rapid saves (configurable 50-500ms)
- **Hash tracking**: SHA-256 hash of file content to detect actual changes vs. no-op saves
- **Watcher pausing**: When writing remote changes to disk, temporarily ignore file events to prevent feedback loops
- **Diff algorithm**: Uses `diff-match-patch` or similar to compute minimal edits, converted to Yjs text operations. Patch-based (not whole-doc replace) for efficiency with concurrent edits.
- **Diff correctness testing**: Both property-based tests (randomized edits, verify CRDT convergence) and golden file tests (curated tricky scenarios: multi-hunk, overlapping, Unicode, empty sections). Golden tests on every commit, property tests nightly.
- **Path safety**: Canonicalize all paths, reject directory traversal and symlink escape attempts
- **Path normalization**: Separators to `/`, Unicode NFKC, reject `.`/`..`, max 512 characters

**Diff-to-Yjs Conversion:**

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

### 5. Git Sync Pipeline

```
┌─────────────────────────────────────────────────────────────────┐
│                    Git Sync Pipeline                              │
│                                                                   │
│  Trigger: User saves OR 30s idle timeout OR manual RPC            │
│       │                                                           │
│       ▼                                                           │
│  Collect all changed files since last commit                      │
│       │                                                           │
│       ▼                                                           │
│  Generate diff summary                                            │
│       │                                                           │
│       ▼                                                           │
│  Call Claude API with diff + context (respecting redaction         │
│  policy: disabled | redacted | full):                             │
│  "Generate a concise git commit message for these changes:        │
│   - Modified: docs/auth.md (rewrote OAuth section)                │
│   - Added: docs/api-keys.md (new document)                        │
│   - Modified: README.md (updated links)"                          │
│       │                                                           │
│       ▼                                                           │
│  Claude returns: "Add API key docs and update OAuth flow          │
│                   with PKCE support"                               │
│  (If AI unavailable: deterministic fallback message)              │
│       │                                                           │
│       ▼                                                           │
│  Create git commit with:                                          │
│  - AI-generated message (or fallback)                             │
│  - Co-authored-by headers for all editors since last commit       │
│  - Scriptum metadata in commit trailer                            │
│       │                                                           │
│       ▼                                                           │
│  Push to configured remote (per push policy:                      │
│  disabled | manual | auto_rebase)                                 │
│                                                                   │
│  Example commit:                                                  │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │ Add API key docs and update OAuth flow with PKCE            │ │
│  │                                                              │ │
│  │ Co-authored-by: Gary <gary@example.com>                      │ │
│  │ Co-authored-by: Claude <agent:claude-1@scriptum>             │ │
│  │ Scriptum-Session: abc123                                     │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

**Git Sync Worker details:**
- **Semantic triggers (primary)**: Section lease release, comment thread resolved, task completed, explicit `scriptum checkpoint` command. These produce meaningful, intent-aligned commits.
- **Idle fallback**: After 30s of inactivity with uncommitted changes, auto-commit as a safety net. Not the primary commit path.
- Max 1 auto-commit per 30s per workspace
- Push policies: `disabled` | `manual` | `auto_rebase`
- AI commit messages: opt-in, redact before API call per workspace policy, deterministic fallback on failure
- **Leader election**: One daemon per workspace is elected git sync leader via relay-mediated protocol. If the leader disconnects, another connected daemon takes over automatically. Prevents multi-writer race conditions (competing commits, forced merges). Leader handles both commit and push operations.

**AI Commit Message Generation:**

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

### 6. Agent Architecture

*(Directly informed by [Niwa](https://github.com/secemp9/niwa)'s agent-first design patterns)*

```
┌─────────────────────────────────────────────────────────────────┐
│                    Agent Integration Layers                       │
│                                                                   │
│  Layer 1: MCP Server (highest fidelity)                           │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │  Tools:                                                      │ │
│  │  - scriptum_read(doc, section?) → content + metadata         │ │
│  │  - scriptum_edit(doc, section, content, summary)             │ │
│  │  - scriptum_list() → workspace documents                     │ │
│  │  - scriptum_tree(doc) → section structure with IDs           │ │
│  │  - scriptum_status() → agent's active sections/overlaps      │ │
│  │  - scriptum_conflicts() → section overlap warnings           │ │
│  │  - scriptum_history(doc) → version timeline                  │ │
│  │  - scriptum_subscribe(doc) → change notifications            │ │
│  │  - scriptum_agents() → list active agents in workspace       │ │
│  │                                                              │ │
│  │  Resources:                                                  │ │
│  │  - scriptum://docs/{id} → document content                   │ │
│  │  - scriptum://docs/{id}/sections → section tree              │ │
│  │  - scriptum://workspace → workspace listing                  │ │
│  │  - scriptum://agents → active agent list                     │ │
│  │                                                              │ │
│  │  All operations go through CRDT → full real-time sync        │ │
│  │  Attribution: agent name from MCP client config               │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                                                                   │
│  Layer 2: CLI (good fidelity)                                     │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │  $ scriptum edit doc.md \                                    │ │
│  │      --section "## Auth" \                                   │ │
│  │      --content "new content" \                               │ │
│  │      --agent claude-1 \                                      │ │
│  │      --summary "Updated OAuth flow"                          │ │
│  │                                                              │ │
│  │  Agent state commands (inspired by Niwa):                    │ │
│  │  $ scriptum whoami          # Suggest unique name            │ │
│  │  $ scriptum status --agent claude-1                          │ │
│  │  $ scriptum conflicts --agent claude-1                       │ │
│  │  $ scriptum agents          # List all active agents         │ │
│  │  $ scriptum tree doc.md     # Section IDs for targeting      │ │
│  │                                                              │ │
│  │  CLI connects to local Scriptum daemon via Unix socket       │ │
│  │  Operations go through CRDT → full real-time sync            │ │
│  │  Attribution: explicit --agent flag                           │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                                                                   │
│  Layer 3: File System Watching (fallback)                         │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │  Agent writes to ~/.scriptum/workspaces/myproject/doc.md     │ │
│  │                                                              │ │
│  │  File watcher detects change                                 │ │
│  │  Diff computed → Yjs operations → synced to peers            │ │
│  │  Attribution: inferred from OS user/process when possible    │ │
│  │  Lower fidelity: paragraph-level diff, no section target     │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

### 7. Agent State Management

Key Niwa insight: agents lose context (sub-agent spawns, context compaction, crashes). The daemon must persist agent state so they can recover.

```
┌─────────────────────────────────────────────────────────────────┐
│              Agent State (persisted in daemon)                    │
│                                                                   │
│  Per agent (keyed by agent name):                                 │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │  agent_id: "claude-1"                                        │ │
│  │  first_seen: 2025-01-15T10:00:00Z                            │ │
│  │  last_seen: 2025-01-15T10:30:00Z                             │ │
│  │                                                              │ │
│  │  active_sections: [                                          │ │
│  │    { doc: "auth.md", section: "h2:oauth", since: ... }       │ │
│  │  ]                                                           │ │
│  │  # Sections this agent has read (registered intent)           │ │
│  │  # Cleared when agent edits the section or times out          │ │
│  │                                                              │ │
│  │  recent_edits: [                                             │ │
│  │    { doc: "auth.md", section: "h2:oauth",                    │ │
│  │      summary: "Added PKCE flow", at: ... }                   │ │
│  │  ]                                                           │ │
│  │  # Last N edits by this agent (for status recovery)           │ │
│  │                                                              │ │
│  │  overlaps: [                                                 │ │
│  │    { doc: "auth.md", section: "h2:oauth",                    │ │
│  │      other_agent: "claude-2", since: ... }                   │ │
│  │  ]                                                           │ │
│  │  # Active section overlaps with other agents                  │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                                                                   │
│  This state survives:                                             │
│  - Agent context switches / sub-agent spawns                      │
│  - Context compaction (/compact in Claude Code)                   │
│  - Agent crashes / restarts                                       │
│  - Daemon restarts (persisted to disk)                            │
│                                                                   │
│  Recovery flow (what a fresh agent does):                         │
│  1. scriptum whoami → get suggested name + workspace summary      │
│  2. scriptum status --agent <name> → recover full state           │
│  3. scriptum conflicts --agent <name> → see any overlaps          │
│  4. Resume editing with full context                              │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

### 8. Claude Code Hooks

*(Directly ported from Niwa's hook system, adapted for Scriptum's CRDT architecture)*

```
┌─────────────────────────────────────────────────────────────────┐
│              Claude Code Hook Integration                         │
│                                                                   │
│  Setup: scriptum setup claude                                     │
│  Creates: .claude/settings.json with hook config                  │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │ SessionStart                                                 │ │
│  │ Trigger: Claude Code session begins                          │ │
│  │ Action:  Inject into context:                                │ │
│  │   - Scriptum CLI quick reference                             │ │
│  │   - Current workspace status (docs, active agents)           │ │
│  │   - This agent's state (if resuming)                         │ │
│  │   - Section overlap warnings (if any)                        │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │ PreCompact                                                   │ │
│  │ Trigger: Before /compact command                             │ │
│  │ Action:  Preserve Scriptum context for post-compaction:      │ │
│  │   - CLI reference (so Claude remembers commands)             │ │
│  │   - Agent name and current state                             │ │
│  │   - Active sections and any overlaps                         │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │ PreToolUse (matcher: Write|Edit on *.md files)               │ │
│  │ Trigger: Claude is about to edit a .md file directly         │ │
│  │ Action:  Provide context:                                    │ │
│  │   - "Consider using `scriptum edit` for better               │ │
│  │     attribution and section-level sync"                      │ │
│  │   - Warn if another agent is editing same section            │ │
│  │   - Show current section state                               │ │
│  │ Note: Does NOT block the edit - just provides context        │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │ PostToolUse (matcher: Write|Edit on *.md files)              │ │
│  │ Trigger: Claude just edited a .md file directly              │ │
│  │ Action:  Confirm sync:                                       │ │
│  │   - "File watcher detected change, synced to CRDT"           │ │
│  │   - Show if any section overlaps resulted from the edit      │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │ Stop                                                         │ │
│  │ Trigger: Claude Code session ending                          │ │
│  │ Action:  Reminder:                                           │ │
│  │   - Any unsynced local changes                               │ │
│  │   - Any active section overlaps to be aware of               │ │
│  │   - "Your agent state is preserved for next session"         │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

### 9. Sync Topology (Hybrid P2P + Relay)

```
┌─────────────────────────────────────────────────────────────────┐
│                    Connection Strategy                            │
│                                                                   │
│  On document open:                                                │
│       │                                                           │
│       ▼                                                           │
│  1. Connect to relay server (y-websocket) — ALWAYS                │
│     - Relay is the persistent CRDT store, auth gateway,           │
│       and awareness aggregator. It is NOT optional.               │
│       │                                                           │
│       ▼                                                           │
│  2. Also try WebRTC (y-webrtc) for direct peer connections        │
│     - Uses signaling server for initial handshake                 │
│     - If peers are on same LAN, use mDNS for zero-config         │
│     - WebRTC is an optimization for lower latency, not            │
│       a primary transport                                         │
│       │                                                           │
│       ▼                                                           │
│  3. Relay server responsibilities:                                │
│     - Persistent CRDT state store (offline catch-up)              │
│     - Awareness aggregator (presence for all peers)               │
│     - Auth gateway (validates workspace access)                   │
│     - Single source of truth when peers disagree                  │
│                                                                   │
│  Optimization:                                                    │
│  - LAN peers discovered via mDNS → direct TCP, lowest            │
│    latency                                                        │
│  - Internet peers → WebRTC when possible for lower latency        │
│  - Relay ALWAYS receives updates (required for persistence)       │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

**Relay Services**: auth, metadata API, sync session manager, update sequencer, snapshot compactor.

**Relay Persistence**: PostgreSQL for metadata + update index, object storage for large snapshots.

---

# Part 3: API Contracts

## Common Conventions

| Convention | Detail |
|---|---|
| Base path | `/v1` |
| Content type | `application/json` |
| Auth | `Authorization: Bearer <access_token>` |
| Idempotency | Mutating `POST` requires `Idempotency-Key` header (except `/auth/*`) |
| Conditional writes | `PATCH`/`DELETE` require `If-Match` header; missing returns `428 Precondition Required` |
| Pagination | Cursor-based: `limit` + opaque `cursor` parameter |
| Error envelope | `{ "error": { "code": "<ERROR_CODE>", "message": "...", "retryable": bool, "request_id": "...", "details": {} } }` |

## Core Object Schemas

```
Workspace {
  id: uuid
  slug: citext
  name: text
  role: text           // caller's role in this workspace
  created_at: timestamptz
  updated_at: timestamptz
  etag: text
}

WorkspaceMember {
  user_id: uuid
  email: citext
  display_name: text
  role: text           // "owner" | "editor" | "viewer"
  status: text         // "active" | "invited" | "suspended"
  joined_at: timestamptz
  last_seen_at: timestamptz
}

Document {
  id: uuid
  workspace_id: uuid
  path: text
  title: text
  tags: text[]
  head_seq: bigint
  etag: text
  archived_at: timestamptz | null
  deleted_at: timestamptz | null
  created_at: timestamptz
  updated_at: timestamptz
}

Section {
  id: text
  parent_id: text | null
  heading: text
  level: int
  start_line: int
  end_line: int
}

CommentThread {
  id: uuid
  doc_id: uuid
  section_id: text | null
  start_offset_utf16: int
  end_offset_utf16: int
  status: text         // "open" | "resolved"
  version: int
  created_by: text
  created_at: timestamptz
  resolved_at: timestamptz | null
}

CommentMessage {
  id: uuid
  thread_id: uuid
  author: text
  body_md: text
  created_at: timestamptz
  edited_at: timestamptz | null
}

ShareLink {
  id: uuid
  target_type: text    // "workspace" | "document"
  target_id: uuid
  permission: text     // "view" | "edit"
  expires_at: timestamptz | null
  max_uses: int | null
  use_count: int
  disabled: bool
  created_at: timestamptz
  revoked_at: timestamptz | null
  url_once: text       // one-time URL, only in creation response
}

SyncSession {
  session_id: uuid
  session_token: text
  ws_url: text
  heartbeat_interval_ms: int  // default 15000
  max_frame_bytes: int        // default 262144
  resume_token: text
  resume_expires_at: timestamptz
}
```

---

## Auth Endpoints

### POST /v1/auth/oauth/github/start
**Auth**: None

**Request**:
```json
{
  "redirect_uri": "https://app.scriptum.dev/callback",
  "state": "random-state-string",
  "code_challenge": "base64url-encoded-challenge",
  "code_challenge_method": "S256"
}
```

**Response 200**:
```json
{
  "flow_id": "uuid",
  "authorization_url": "https://github.com/login/oauth/authorize?...",
  "expires_at": "2025-01-15T10:15:00Z"
}
```

**Errors**: `AUTH_INVALID_REDIRECT` (400), `RATE_LIMITED` (429)

### POST /v1/auth/oauth/github/callback
**Auth**: None

**Request**:
```json
{
  "flow_id": "uuid",
  "code": "github-oauth-code",
  "state": "random-state-string",
  "code_verifier": "original-verifier",
  "device_name": "Gary's MacBook Pro"
}
```

**Response 200**:
```json
{
  "access_token": "jwt-access-token",
  "access_expires_at": "2025-01-15T10:15:00Z",
  "refresh_token": "opaque-refresh-token",
  "refresh_expires_at": "2025-02-14T10:00:00Z",
  "user": {
    "id": "uuid",
    "email": "gary@example.com",
    "display_name": "Gary"
  }
}
```

**Errors**: `AUTH_STATE_MISMATCH` (401), `AUTH_CODE_INVALID` (401)

### POST /v1/auth/token/refresh
**Auth**: None

**Request**:
```json
{
  "refresh_token": "opaque-refresh-token"
}
```

**Response 200**: Same token payload as callback.

**Errors**: `AUTH_INVALID_TOKEN` (401), `AUTH_TOKEN_REVOKED` (401)

### POST /v1/auth/logout
**Auth**: Bearer token

**Request**:
```json
{
  "refresh_token": "opaque-refresh-token"
}
```

**Response**: `204 No Content`

---

## Workspace & Membership Endpoints

### POST /v1/workspaces
**Request**: `{ "name": "My Project", "slug": "my-project" }`
**Response 201**: `{ "workspace": Workspace }`

### GET /v1/workspaces?limit=50&cursor=
**Response 200**: `{ "items": [Workspace], "next_cursor": "..." }`

### GET /v1/workspaces/{id}
**Response 200**: `{ "workspace": Workspace }`

### PATCH /v1/workspaces/{id}
**Headers**: `If-Match: "etag"`
**Request**: `{ "name": "New Name", "slug": "new-slug" }`
**Response 200**: `{ "workspace": Workspace }`

### POST /v1/workspaces/{id}/invites
**Request**: `{ "email": "dev@example.com", "role": "editor", "expires_at": "2025-02-15T00:00:00Z" }`
**Response 201**: `{ "invite_id": "uuid", "email": "dev@example.com", "role": "editor", "expires_at": "...", "status": "pending" }`

### POST /v1/invites/{token}/accept
**Request**: `{ "display_name": "Dev User" }`
**Response 200**: `{ "workspace": Workspace, "member": WorkspaceMember }`

### GET /v1/workspaces/{id}/members
**Response 200**: Paged `WorkspaceMember` list.

### PATCH /v1/workspaces/{id}/members/{user_id}
**Headers**: `If-Match: "etag"`
**Request**: `{ "role": "viewer" }` or `{ "status": "suspended" }`
**Response 200**: `WorkspaceMember`

### DELETE /v1/workspaces/{id}/members/{user_id}
**Headers**: `If-Match: "etag"`
**Response**: `204 No Content`

---

## Document Endpoints

### GET /v1/workspaces/{id}/documents?limit=100&cursor=&path_prefix=&tag=&include_archived=false
**Response 200**: `{ "items": [Document], "next_cursor": "..." }`

### POST /v1/workspaces/{id}/documents
**Request**:
```json
{
  "path": "docs/auth.md",
  "title": "Authentication Guide",
  "content_md": "# Authentication\n\nContent here...",
  "tags": ["rfc", "auth"]
}
```
**Response 201**: `{ "document": Document, "sections": [Section], "etag": "..." }`

### GET /v1/workspaces/{id}/documents/{doc_id}?include_content=true&include_sections=true
**Response 200**: `{ "document": Document, "content_md": "...", "sections": [Section] }`

### PATCH /v1/workspaces/{id}/documents/{doc_id}
**Headers**: `If-Match: "etag"`
**Request**: `{ "title": "New Title", "path": "docs/new-path.md", "archived": true }`
**Response 200**: `{ "document": Document }`

### DELETE /v1/workspaces/{id}/documents/{doc_id}?hard_delete=false
**Headers**: `If-Match: "etag"`
**Response**: `204 No Content`

### POST /v1/workspaces/{id}/documents/{doc_id}/tags
**Request**: `{ "op": "add", "tags": ["approved"] }`
**Response 200**: `{ "document": Document }`

### GET /v1/workspaces/{id}/search?q=authentication&limit=50&cursor=
**Response 200**: `{ "items": [{ "doc_id": "uuid", "path": "...", "title": "...", "snippet": "...", "score": 0.95 }], "next_cursor": "..." }`

---

## Comment Endpoints

### GET /v1/workspaces/{id}/documents/{doc_id}/comments?status=open&limit=100&cursor=
**Response 200**: Paged list of `CommentThread` with messages.

### POST /v1/workspaces/{id}/documents/{doc_id}/comments
**Request**:
```json
{
  "anchor": {
    "section_id": "h2:authentication",
    "start_offset_utf16": 42,
    "end_offset_utf16": 128,
    "head_seq": 15
  },
  "message": "Should we use PKCE here?"
}
```
**Response 201**: `{ "thread": CommentThread, "message": CommentMessage }`

### POST /v1/workspaces/{id}/comments/{id}/messages
**Request**: `{ "body_md": "Yes, PKCE is required for public clients." }`
**Response 201**: `{ "message": CommentMessage }`

### POST /v1/workspaces/{id}/comments/{id}/resolve
**Request**: `{ "if_version": 3 }`
**Response 200**: `{ "thread": CommentThread }`

### POST /v1/workspaces/{id}/comments/{id}/reopen
**Request**: `{ "if_version": 4 }`
**Response 200**: `{ "thread": CommentThread }`

---

## Share Link & ACL Override Endpoints

### POST /v1/workspaces/{id}/documents/{doc_id}/acl-overrides
**Request**: `{ "subject_type": "user", "subject_id": "uuid", "role": "editor", "expires_at": "2025-03-01T00:00:00Z" }`
**Response 201**: `{ "acl_override": AclOverride }`

### DELETE /v1/workspaces/{id}/documents/{doc_id}/acl-overrides/{override_id}
**Response**: `204 No Content`

### POST /v1/workspaces/{id}/share-links
**Request**:
```json
{
  "target_type": "document",
  "target_id": "doc-uuid",
  "permission": "view",
  "expires_at": "2025-03-01T00:00:00Z",
  "max_uses": 10,
  "password": "optional-password"
}
```
**Response 201**: `{ "share_link": ShareLink }` (includes one-time `url_once` field)

### PATCH /v1/workspaces/{id}/share-links/{id}
**Headers**: `If-Match: "etag"`
**Request**: `{ "permission": "edit", "expires_at": "2025-04-01T00:00:00Z", "max_uses": 50, "disabled": false }`
**Response 200**: `{ "share_link": ShareLink }`

---

## Sync Session & WebSocket Protocol

### POST /v1/workspaces/{id}/sync-sessions
**Request**:
```json
{
  "protocol": "scriptum-sync.v1",
  "client_id": "uuid",
  "device_id": "uuid",
  "resume_token": "optional-previous-token"
}
```
**Response 201**:
```json
{
  "session_id": "uuid",
  "session_token": "session-jwt",
  "ws_url": "wss://relay.scriptum.dev/v1/ws/abc123",
  "heartbeat_interval_ms": 15000,
  "max_frame_bytes": 262144,
  "resume_token": "opaque-resume-token",
  "resume_expires_at": "2025-01-15T10:10:00Z"
}
```

### WebSocket Protocol (`scriptum-sync.v1`)

**Connection**: Frame limit max 262,144 bytes. Heartbeat ping every 15s; disconnect if no pong within 10s.

**Resume token**: TTL 10 min, single-use, bound to `(session_id, workspace_id, client_id, device_id)`.

**Message types**:

| Type | Direction | Fields |
|------|-----------|--------|
| `hello` | client -> server | `type`, `session_token`, `resume_token?` |
| `hello_ack` | server -> client | `type`, `server_time`, `resume_accepted` |
| `subscribe` | client -> server | `type`, `doc_id`, `last_server_seq?` |
| `yjs_update` | bidirectional | `type`, `doc_id`, `client_id`, `client_update_id`, `base_server_seq`, `payload_b64` |
| `ack` | server -> client | `type`, `doc_id`, `client_update_id`, `server_seq`, `applied` |
| `snapshot` | server -> client | `type`, `doc_id`, `snapshot_seq`, `payload_b64` |
| `awareness_update` | bidirectional | `type`, `doc_id`, `peers: [...]` |
| `error` | server -> client | `type`, `code`, `message`, `retryable`, `doc_id?` |

**Dedupe**: Client sends `(client_id, client_update_id UUIDv7)` with every update. Server deduplicates by this key. At-least-once delivery; idempotent apply.

---

## Daemon JSON-RPC Contract (`scriptum-daemon.v1`)

JSON-RPC 2.0 over local Unix socket (`~/.scriptum/daemon.sock`) or Windows named pipe (`\\.\pipe\scriptum-daemon`).

### Workspace Methods

**`workspace.list`**
- Params: `{ limit?: int, cursor?: string }`
- Result: `{ items: [Workspace], next_cursor: string | null }`

**`workspace.open`**
- Params: `{ workspace_id: string }`
- Result: `{ workspace: Workspace, root_path: string }`

**`workspace.create`**
- Params: `{ name: string, root_path: string }`
- Result: `{ workspace: Workspace }`

### Document Methods

**`doc.read`**
- Params: `{ workspace_id: string, doc_id: string, include_content?: bool }`
- Result: `{ document: Document, content_md?: string, sections: [Section] }`

**`doc.edit`**
- Params: `{ workspace_id: string, doc_id: string, client_update_id: string, ops?: YjsOps, content_md?: string, if_etag?: string, agent_id?: string }`
- Result: `{ etag: string, head_seq: int }`

**`doc.sections`**
- Params: `{ workspace_id: string, doc_id: string }`
- Result: `{ sections: [Section] }`

**`doc.tree`**
- Params: `{ workspace_id: string, path_prefix?: string }`
- Result: `{ items: [{ path: string, doc_id: string, title: string }] }`

**`doc.search`**
- Params: `{ workspace_id: string, q: string, limit?: int, cursor?: string }`
- Result: `{ items: [{ doc_id: string, path: string, title: string, snippet: string, score: float }], next_cursor: string | null }`

**`doc.diff`**
- Params: `{ workspace_id: string, doc_id: string, from_seq: int, to_seq: int }`
- Result: `{ patch_md: string }`

### Agent Methods

**`agent.whoami`**
- Params: `{}`
- Result: `{ agent_id: string, capabilities: [string] }`

**`agent.status`**
- Params: `{ workspace_id: string }`
- Result: `{ active_sessions: [AgentSession] }`

**`agent.conflicts`**
- Params: `{ workspace_id: string, doc_id?: string }`
- Result: `{ items: [SectionOverlap] }`

**`agent.list`**
- Params: `{ workspace_id: string }`
- Result: `{ items: [{ agent_id: string, last_seen_at: string, active_sections: int }] }`

**`agent.claim`**
- Params: `{ workspace_id: string, doc_id: string, section_id: string, ttl_sec: int, mode: "exclusive" | "shared", note?: string }`
- Result: `{ lease_id: string, expires_at: string, conflicts: [{ agent_id: string, section_id: string }] }`
- Note: Leases are advisory only (V1). Other editors see a warning but are not blocked.

### Document Bundle Methods

**`doc.bundle`**
- Params: `{ workspace_id: string, doc_id: string, section_id?: string, include: ("parents" | "children" | "backlinks" | "comments")[], token_budget?: int }`
- Result: `{ section_content: string, context: { parents: [Section], children: [Section], backlinks: [{ doc_id, path, snippet }], comments: [CommentThread] }, tokens_used: int }`

### Git Methods

**`git.status`**
- Params: `{ workspace_id: string }`
- Result: `{ branch: string, dirty: bool, ahead: int, behind: int, last_sync_at: string | null }`

**`git.sync`**
- Params: `{ workspace_id: string, mode: "commit" | "commit_and_push", agent_id?: string }`
- Result: `{ job_id: string, status: "queued" | "running" | "completed" | "failed" }`

**`git.configure`**
- Params: `{ workspace_id: string, policy: GitSyncPolicy }`
- Result: `{ policy: GitSyncPolicy }`

---

## MCP Tool Contract

MCP tools mirror the daemon JSON-RPC interface. Each tool connects to `scriptumd` via the local socket.

| MCP Tool | Daemon RPC | Description |
|----------|-----------|-------------|
| `scriptum_read` | `doc.read` | Read document content, optionally scoped to section |
| `scriptum_edit` | `doc.edit` | Apply content edits with attribution |
| `scriptum_list` | `doc.tree` | List workspace documents |
| `scriptum_tree` | `doc.sections` | Show document section structure with IDs |
| `scriptum_status` | `agent.status` | Agent's active sessions and sections |
| `scriptum_conflicts` | `agent.conflicts` | Section overlap warnings |
| `scriptum_history` | `doc.diff` | Version timeline / diffs |
| `scriptum_subscribe` | (notification channel) | Change notifications for watched docs |
| `scriptum_agents` | `agent.list` | List active agents in workspace |
| `scriptum_claim` | `agent.claim` | Claim a section with advisory lease (TTL, mode, note) |
| `scriptum_bundle` | `doc.bundle` | Get optimally-sized context bundle for a section |

**MCP Resources**:
- `scriptum://docs/{id}` -- document content
- `scriptum://docs/{id}/sections` -- section tree
- `scriptum://workspace` -- workspace listing
- `scriptum://agents` -- active agent list

---

# Part 4: Data Models

## Local Storage Directory Layout

```
~/.scriptum/
├── config.toml                  # Global config (API keys, defaults)
├── workspaces/
│   ├── my-project/
│   │   ├── .scriptum/
│   │   │   ├── workspace.toml   # Workspace config (git remote, sync settings)
│   │   │   ├── crdt_store/      # CRDT persistence
│   │   │   │   ├── wal/         # Append-only write-ahead log
│   │   │   │   │   ├── doc1.wal
│   │   │   │   │   └── doc2.wal
│   │   │   │   └── snapshots/   # Compressed snapshots
│   │   │   │       ├── doc1.snap
│   │   │   │       └── doc2.snap
│   │   │   ├── meta.db          # SQLite: document metadata, agent state
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

## Relay Server Storage Layout

```
Relay Server:
├── PostgreSQL                   # All relay metadata (see schema below)
│   ├── users, workspaces, documents, etc.
│   └── yjs_update_log, yjs_snapshots
├── Object Storage (S3/R2)       # Large CRDT snapshots
│   └── {workspace_id}/{doc_id}/{snapshot_seq}.snap
└── (awareness is ephemeral, in-memory only)
```

---

## Relay Database Schema (PostgreSQL 15+)

```sql
-- ============================================================
-- Users & Auth
-- ============================================================

CREATE TABLE users (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    email           citext UNIQUE NOT NULL,
    display_name    text NOT NULL,
    password_hash   text NULL,          -- Argon2id, optional (OAuth users may not have)
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE refresh_sessions (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         uuid NOT NULL REFERENCES users(id),
    token_hash      bytea UNIQUE NOT NULL,
    family_id       uuid NOT NULL,      -- rotation family for reuse detection
    rotated_from    uuid NULL,          -- previous session in family
    expires_at      timestamptz NOT NULL,
    revoked_at      timestamptz NULL,
    created_at      timestamptz NOT NULL DEFAULT now()
);

-- ============================================================
-- Workspaces & Membership
-- ============================================================

CREATE TABLE workspaces (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    slug            citext UNIQUE NOT NULL,
    name            text NOT NULL,
    created_by      uuid NOT NULL REFERENCES users(id),
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz NULL
);

CREATE TABLE workspace_members (
    workspace_id    uuid NOT NULL REFERENCES workspaces(id),
    user_id         uuid NOT NULL REFERENCES users(id),
    role            text NOT NULL CHECK (role IN ('owner', 'editor', 'viewer')),
    status          text NOT NULL CHECK (status IN ('active', 'invited', 'suspended')),
    joined_at       timestamptz NOT NULL DEFAULT now(),
    last_seen_at    timestamptz NULL,
    PRIMARY KEY (workspace_id, user_id)
);

-- ============================================================
-- Documents & Organization
-- ============================================================

CREATE TABLE documents (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id    uuid NOT NULL REFERENCES workspaces(id),
    path            text NOT NULL,
    path_norm       text NOT NULL,      -- normalized for uniqueness checks
    title           text NULL,
    head_seq        bigint NOT NULL DEFAULT 0,
    etag            text NOT NULL,
    created_by      uuid NOT NULL REFERENCES users(id),
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    archived_at     timestamptz NULL,
    deleted_at      timestamptz NULL,
    UNIQUE (workspace_id, path_norm) WHERE (deleted_at IS NULL)  -- partial unique
);

CREATE INDEX idx_documents_workspace_updated ON documents (workspace_id, updated_at DESC);
CREATE INDEX idx_documents_workspace_path    ON documents (workspace_id, path_norm);

CREATE TABLE tags (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id    uuid NOT NULL REFERENCES workspaces(id),
    name            text NOT NULL,
    color           text NULL,
    UNIQUE (workspace_id, name)
);

CREATE TABLE document_tags (
    document_id     uuid NOT NULL REFERENCES documents(id),
    tag_id          uuid NOT NULL REFERENCES tags(id),
    PRIMARY KEY (document_id, tag_id)
);

CREATE TABLE backlinks (
    workspace_id    uuid NOT NULL REFERENCES workspaces(id),
    src_doc_id      uuid NOT NULL REFERENCES documents(id),
    dst_doc_id      uuid NOT NULL REFERENCES documents(id),
    anchor_text     text NULL,
    PRIMARY KEY (workspace_id, src_doc_id, dst_doc_id)
);

-- ============================================================
-- Comments
-- ============================================================

CREATE TABLE comment_threads (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id        uuid NOT NULL REFERENCES workspaces(id),
    doc_id              uuid NOT NULL REFERENCES documents(id),
    section_id          text NULL,
    start_offset_utf16  int NULL,
    end_offset_utf16    int NULL,
    status              text NOT NULL CHECK (status IN ('open', 'resolved')) DEFAULT 'open',
    version             int NOT NULL DEFAULT 1,
    created_by_user_id  uuid NULL REFERENCES users(id),
    created_by_agent_id text NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),
    resolved_at         timestamptz NULL
);

CREATE INDEX idx_comment_threads_doc_status ON comment_threads (workspace_id, doc_id, status);

CREATE TABLE comment_messages (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    thread_id           uuid NOT NULL REFERENCES comment_threads(id),
    author_user_id      uuid NULL REFERENCES users(id),
    author_agent_id     text NULL,
    body_md             text NOT NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),
    edited_at           timestamptz NULL
);

-- ============================================================
-- Sharing & Access Control
-- ============================================================

CREATE TABLE share_links (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id    uuid NOT NULL REFERENCES workspaces(id),
    target_type     text NOT NULL CHECK (target_type IN ('workspace', 'document')),
    target_id       uuid NOT NULL,
    permission      text NOT NULL CHECK (permission IN ('view', 'edit')),
    token_hash      bytea NOT NULL,     -- hashed share token (128-bit random)
    password_hash   text NULL,          -- optional Argon2id password
    expires_at      timestamptz NULL,
    max_uses        int NULL,
    use_count       int NOT NULL DEFAULT 0,
    disabled        bool NOT NULL DEFAULT false,
    created_by      uuid NOT NULL REFERENCES users(id),
    created_at      timestamptz NOT NULL DEFAULT now(),
    revoked_at      timestamptz NULL
);

CREATE TABLE acl_overrides (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id    uuid NOT NULL REFERENCES workspaces(id),
    doc_id          uuid NOT NULL REFERENCES documents(id),
    subject_type    text NOT NULL CHECK (subject_type IN ('user', 'agent', 'share_link')),
    subject_id      text NOT NULL,
    role            text NOT NULL CHECK (role IN ('editor', 'viewer')),
    expires_at      timestamptz NULL,
    created_at      timestamptz NOT NULL DEFAULT now()
);

-- ============================================================
-- CRDT Sync
-- ============================================================

CREATE TABLE yjs_update_log (
    workspace_id        uuid NOT NULL,
    doc_id              uuid NOT NULL,
    server_seq          bigint NOT NULL,
    client_id           uuid NOT NULL,
    client_update_id    uuid NOT NULL,
    payload             bytea NOT NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, doc_id, server_seq),
    UNIQUE (workspace_id, doc_id, client_id, client_update_id)
);

CREATE INDEX idx_yjs_update_log_created ON yjs_update_log (workspace_id, doc_id, created_at DESC);

CREATE TABLE yjs_snapshots (
    workspace_id    uuid NOT NULL,
    doc_id          uuid NOT NULL,
    snapshot_seq    bigint NOT NULL,
    payload         bytea NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, doc_id, snapshot_seq)
);

-- ============================================================
-- Idempotency & Infrastructure
-- ============================================================

CREATE TABLE idempotency_keys (
    scope           text NOT NULL,
    idem_key        text NOT NULL,
    request_hash    bytea NOT NULL,
    response_status int NOT NULL,
    response_body   jsonb NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    expires_at      timestamptz NOT NULL,
    PRIMARY KEY (scope, idem_key)
);

-- ============================================================
-- Audit
-- ============================================================

CREATE TABLE audit_events (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id    uuid NULL,
    actor_user_id   uuid NULL,
    actor_agent_id  text NULL,
    event_type      text NOT NULL,
    entity_type     text NOT NULL,
    entity_id       text NOT NULL,
    request_id      text NULL,
    ip_hash         bytea NULL,         -- hashed, not raw IP
    user_agent_hash bytea NULL,
    details         jsonb NULL,
    created_at      timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX idx_audit_events_workspace ON audit_events (workspace_id, created_at DESC);
```

**Retention policy**:
- Keep updates 90 days.
- Snapshot every 1,000 updates or 10 min (whichever comes first).
- Compact updates older than latest-2 snapshots.
- Audit: 180 days hot, archive per tenant.

---

## Local Daemon Database Schema (`meta.db`, SQLite)

```sql
-- ============================================================
-- Document tracking
-- ============================================================

CREATE TABLE documents_local (
    doc_id              text PRIMARY KEY,
    workspace_id        text NOT NULL,
    abs_path            text NOT NULL,
    line_ending_style   text NOT NULL,      -- "lf" | "crlf" | "mixed"
    last_fs_mtime_ns    integer NOT NULL,
    last_content_hash   text NOT NULL,      -- SHA-256
    projection_rev      integer NOT NULL,   -- local projection version
    last_server_seq     integer NOT NULL DEFAULT 0,
    last_ack_seq        integer NOT NULL DEFAULT 0,
    parse_error         text NULL
);

-- ============================================================
-- Agent state
-- ============================================================

CREATE TABLE agent_sessions (
    session_id      text PRIMARY KEY,
    agent_id        text NOT NULL,
    workspace_id    text NOT NULL,
    started_at      text NOT NULL,
    last_seen_at    text NOT NULL,
    status          text NOT NULL       -- "active" | "idle" | "disconnected"
);

CREATE TABLE agent_recent_edits (
    id                  integer PRIMARY KEY AUTOINCREMENT,
    doc_id              text NOT NULL,
    agent_id            text NOT NULL,
    start_offset_utf16  integer NOT NULL,
    end_offset_utf16    integer NOT NULL,
    ts                  text NOT NULL
);

-- ============================================================
-- Git sync
-- ============================================================

CREATE TABLE git_sync_config (
    workspace_id        text PRIMARY KEY,
    mode                text NOT NULL,      -- "auto" | "manual" | "disabled"
    remote_name         text NOT NULL DEFAULT 'origin',
    branch              text NOT NULL DEFAULT 'main',
    commit_interval_sec integer NOT NULL DEFAULT 30,
    push_policy         text NOT NULL DEFAULT 'disabled',  -- "disabled" | "manual" | "auto_rebase"
    ai_enabled          integer NOT NULL DEFAULT 1,
    redaction_policy    text NOT NULL DEFAULT 'redacted'   -- "disabled" | "redacted" | "full"
);

CREATE TABLE git_sync_jobs (
    job_id              text PRIMARY KEY,
    workspace_id        text NOT NULL,
    state               text NOT NULL,      -- "queued" | "running" | "completed" | "failed"
    attempt_count       integer NOT NULL DEFAULT 0,
    next_attempt_at     text NULL,
    last_error_code     text NULL,
    last_error_message  text NULL,
    created_at          text NOT NULL,
    updated_at          text NOT NULL
);

-- ============================================================
-- Sync outbox
-- ============================================================

CREATE TABLE outbox_updates (
    id                  integer PRIMARY KEY AUTOINCREMENT,
    workspace_id        text NOT NULL,
    doc_id              text NOT NULL,
    client_update_id    text NOT NULL,
    payload             blob NOT NULL,
    retry_count         integer NOT NULL DEFAULT 0,
    next_retry_at       text NULL,
    state               text NOT NULL DEFAULT 'pending',  -- "pending" | "sent" | "acked" | "dead"
    created_at          text NOT NULL
);

-- ============================================================
-- Schema versioning
-- ============================================================

CREATE TABLE schema_migrations (
    version     integer PRIMARY KEY,
    applied_at  text NOT NULL
);
```

---

## Local Document Metadata Schema (legacy/extended)

This schema extends `meta.db` with higher-level document metadata for the desktop/web app:

```sql
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

## CRDT Storage Layout & Retention

```
.scriptum/crdt_store/
├── wal/
│   ├── {doc_id}.wal           # Append-only write-ahead log
│   │   Format: [length:u32][checksum:u32][yjs_update:bytes]*
│   │   - New Yjs updates appended atomically
│   │   - fsync before acknowledging local write
│   └── ...
├── snapshots/
│   ├── {doc_id}.snap          # Compressed Yjs state snapshot
│   │   - Created every 1000 updates or 10 min
│   │   - Contains full merged Yjs document state
│   └── ...
└── (lock files for concurrent access)

Recovery procedure:
1. Load latest snapshot for document
2. Replay WAL entries after snapshot's sequence number
3. If checksum validation fails, mark document as degraded
4. Degraded documents still accept writes but show warning in UI
```

**Retention (relay)**:
- Keep updates for 90 days.
- Snapshot every 1,000 updates or 10 minutes.
- Compact (delete) updates older than the latest-2 snapshots.
- Audit events: 180 days hot storage, archive per tenant beyond that.

---

# Part 5: Operations

## Security Considerations

### Authentication
- **OAuth 2.1 + PKCE**: Primary auth flow for web and desktop. GitHub OAuth as initial provider.
- **Optional password**: Argon2id hashed, for users who prefer email/password.
- **Access tokens**: 15-minute JWT, workspace-scoped.
- **Refresh tokens**: 30-day rotating, single-use. Stored as hashed values. Reuse detection revokes entire token family.
- **Desktop auth**: OAuth device flow or browser-redirect flow via Tauri deep links.
- **Agent auth**: CLI/MCP connect to local daemon; daemon authenticates to relay with stored credentials.

### Authorization
- **Workspace RBAC**: Owner, Editor, Viewer roles enforced on every REST route and WebSocket `subscribe`.
- **Document ACL overrides**: Per-document role grants for specific users/agents, with optional expiry.
- **Enforcement point**: Relay server checks permissions before processing any operation.

### Share Links
- 128-bit random token, stored hashed (never in plaintext).
- Optional Argon2id password protection.
- Bounded by: max uses, expiry time, disabled flag.
- Rate limiting on redeem endpoint to prevent brute-force.

### Input Validation
- Strict JSON schema validation on all endpoints.
- REST body max 1 MiB.
- WebSocket frame max 256 KiB.
- Path canonicalization on all file operations.
- Markdown XSS sanitization allowlist for rendered content.

### Transport & Storage
- TLS 1.2+ required (1.3 preferred) for all relay connections.
- Encryption at rest for stored CRDT data and database.
- OS keychain for local secrets (API keys, git credentials, tokens) -- never in config files.
- Local CRDT state files readable only by owning user (0600 permissions).

### Audit
- Immutable append-only audit events for: auth, permission changes, share link operations, deletes, admin actions.
- PII minimized: IP addresses and user agents stored as hashes, not raw values.
- Retention: 180 days hot, archive per tenant configuration.

### Encryption Model
- **V1: Server-readable**. Relay can read document content. This enables: server-side search, share link rendering, server-side AI features, and easier debugging. Standard SaaS model.
- **V2: E2EE opt-in**. Storage layer is designed so end-to-end encryption can be layered on per-workspace for enterprise/privacy-sensitive users. When E2EE is enabled: relay stores opaque blobs, server-side search is disabled, share links require client-side decryption.
- **Migration path**: E2EE workspaces are a separate mode, not a global toggle. Existing server-readable workspaces are unaffected.

### AI Safety
- Per-workspace redaction policy for AI commit messages: `disabled` (no AI) | `redacted` (sanitized diff) | `full` (complete diff).
- Redaction strips sensitive patterns (API keys, secrets, credentials) before sending to Claude API.

---

## Error Handling Strategy

### Error Code Registry

| Error Code | HTTP Status | Description | Retryable |
|---|---|---|---|
| `VALIDATION_FAILED` | 400 | Request body fails schema validation | No |
| `IDEMPOTENCY_KEY_REQUIRED` | 400 | Mutating POST missing Idempotency-Key header | No |
| `AUTH_INVALID_TOKEN` | 401 | Access token expired, malformed, or invalid | No (refresh) |
| `AUTH_TOKEN_REVOKED` | 401 | Refresh token has been revoked | No (re-auth) |
| `AUTH_STATE_MISMATCH` | 401 | OAuth state parameter mismatch | No |
| `AUTH_CODE_INVALID` | 401 | OAuth authorization code invalid or expired | No |
| `SYNC_TOKEN_EXPIRED` | 401 | WebSocket session token expired | No (new session) |
| `AUTH_FORBIDDEN` | 403 | Caller lacks required role/permission | No |
| `NOT_FOUND` | 404 | Resource does not exist or caller cannot see it | No |
| `DOC_PATH_CONFLICT` | 409 | Document path already exists in workspace | No |
| `GIT_PUSH_REJECTED` | 409 | Git remote rejected push (non-fast-forward) | Yes (rebase) |
| `IDEMPOTENCY_REPLAY_MISMATCH` | 409 | Same idempotency key with different request body | No |
| `EDIT_PRECONDITION_FAILED` | 412 | `if_etag` or `if_version` does not match current state | No (re-read) |
| `YJS_UPDATE_TOO_LARGE` | 413 | Yjs update payload exceeds max frame size | No |
| `PRECONDITION_REQUIRED` | 428 | PATCH/DELETE missing required `If-Match` header | No |
| `RATE_LIMITED` | 429 | Too many requests; check `Retry-After` header | Yes |
| `INTERNAL_ERROR` | 500 | Unexpected server error | Yes |
| `AI_COMMIT_UNAVAILABLE` | 503 | Claude API unreachable; deterministic fallback used | Yes |
| `DISK_WRITE_FAILED` | 507 | Local daemon failed to write to disk | Yes |

### Retry Policy
- **Transient errors** (429, 500, 503): Exponential backoff 250ms..30s, max 8 attempts.
- **Dead letter**: After exceeding retry policy thresholds, updates are moved to dead-letter queue for manual intervention.
- **Deterministic fallback**: When AI commit generation fails, use structured fallback message: `"Update {n} file(s): {paths}"`.

---

## Performance Requirements / SLAs

**Benchmark assumptions**: M2 16GB client, 4vCPU/16GB relay + dedicated PostgreSQL, 40ms RTT, <=4KiB median / <=32KiB p95 update payloads.

| Metric | Target | Approach |
|--------|--------|----------|
| Editor keystroke latency | < 16ms | Local-first CRDT, async sync |
| Local edit apply (500KB doc) | <= 12ms p95 | Yjs binary encoding, Rust y-crdt |
| File watcher response (<=256KB) | <= 250ms p95 | fsevents (macOS) / inotify (Linux), debounce |
| Client-to-relay ack (same region) | <= 500ms p95 | WebSocket, binary encoding |
| CRDT sync latency (P2P) | < 100ms | WebRTC data channels |
| CRDT sync latency (relay) | < 300ms | WebSocket, binary encoding |
| Reconnect catch-up (10k updates) | <= 2s p95 | Snapshot within 1000 updates |
| Metadata API | <= 200ms p95, <= 500ms p99 | PostgreSQL with proper indexing |
| Git commit generation | < 2s | Claude Haiku for commit messages (fast, cheap) |
| Desktop app startup | < 1s | Tauri, lazy document loading |
| Desktop app memory | < 100MB | Rust backend, efficient CRDT encoding |
| Web app initial load | < 2s | Code splitting, edge CDN |
| Relay availability | >= 99.9% monthly | Multi-AZ deployment, health checks |
| Sync success rate | >= 99.95% monthly | At-least-once delivery, outbox queue |
| Capacity per pod | 1000 concurrent sessions | Horizontal scaling behind load balancer |
| Sustained throughput | 50 updates/sec/doc | Sequencer with batched writes |

---

## Observability

### Logging
- **Format**: Structured JSON logs.
- **Fields**: `request_id`, `workspace_id`, actor hash (not raw PII), endpoint, error code, latency.
- **Retention**: 30 days hot, 180 days cold/archived.

### Metrics
- **RED metrics**: Rate, Errors, Duration for all API and WebSocket endpoints.
- **Custom metrics**:
  - `sync_ack_latency_ms` -- time from client update to server ack
  - `outbox_depth` -- pending updates per workspace
  - `daemon_recovery_time_ms` -- time to reload state on crash recovery
  - `git_sync_jobs_total` -- by state (queued/running/completed/failed)
  - `sequence_gap_count` -- detected gaps in server_seq ordering

### Tracing
- **OpenTelemetry** spans across API handler, sequencer, DB queries, object storage operations.
- Trace ID propagated through REST, WebSocket, and daemon JSON-RPC calls.

### Alerting

| Alert | Condition | Severity | Response |
|-------|-----------|----------|----------|
| Availability drop | < 99.5% over 15 min window | Page | Runbook: check health endpoints, scale pods |
| Sync error spike | > 1% sync errors over 10 min | Page | Runbook: check sequencer, DB connections |
| Outbox growth | 10x growth over 15 min | Page | Runbook: check relay connectivity, outbox backpressure |
| Latency breach | p95 exceeds target for 3 hours | Ticket | Runbook: profile hot paths, check DB queries |

Every paging alert must have an associated runbook before going to production.

---

## Testing Strategy

| Category | Scope | Frequency |
|----------|-------|-----------|
| **Unit** | Parser, sequencer, auth, ACL logic | Every commit |
| **Property** | CRDT convergence (10k-op randomized scenarios), diff-to-Yjs correctness (randomized file edits → verify CRDT state) | Nightly |
| **Golden file** | Diff-to-Yjs edge cases: multi-hunk patches, overlapping edits, Unicode, empty sections, large replacements | Every commit |
| **Integration** | Daemon+watcher, relay+WS, git worker, snapshot/recovery | Every PR |
| **Contract** | REST vs OpenAPI spec, WS frame schemas, JSON-RPC, MCP parity | Every PR |
| **UI visual** | Playwright screenshot baselines for key screens (editor, sidebar, presence, overlaps, sync status). Chromium canonical; strict pixel threshold. | Every PR |
| **UI structural** | Playwright ARIA snapshots for all views — machine-readable accessibility tree baselines that agents can diff textually | Every PR |
| **UI layout/token** | Computed geometry + CSS token assertions for critical elements (spacing, typography, design system compliance) | Every PR |
| **Security** | AuthZ bypass, path traversal, XSS, token replay, brute-force | Every PR + periodic pen test |
| **Load** | 1000 sessions, 50 updates/sec/doc, 1hr soak | Weekly / pre-release |
| **Compatibility** | N and N-1 client/server version matrix | Every release |

**Release gate**: No P0/P1 bugs open, contract and security tests at 100%, SLO benchmarks pass, UI visual + structural baselines pass.

### UI Testing & Visual Regression

The highest-risk UI surfaces in Scriptum (CodeMirror hybrid live preview, presence overlays, CRDT sync states) require more than unit tests — they need visual and structural verification that agents can run in a tight loop.

**Three correctness contracts:**

1. **Visual (pixel regression)**: Screenshot baselines for every key screen state. Playwright `toHaveScreenshot()` against Chromium as the canonical environment. Strict pixel threshold (zero diff for canonical, slightly looser for secondary engines if needed).
2. **Structural (ARIA tree)**: Playwright ARIA snapshots (`toMatchAriaSnapshot`) for every view. Produces a stable YAML artifact that agents can reason about textually — no image inspection needed. Also enforces accessibility correctness.
3. **Layout/design tokens**: A small Playwright helper extracts a JSON "layout contract" for critical elements (bounding boxes, font size/weight, key Tailwind CSS variables) and snapshots it with `toMatchSnapshot`. Catches "button shifted 4px" issues without relying purely on pixel diffs, and gives agents an interpretable textual diff.

**Fixture mode (test-only):**

The app exposes a fixture mode (enabled via environment variable, stripped from production builds) that makes UI states fully deterministic:

- Disables all CSS animations/transitions and CodeMirror caret blinking
- Freezes `Date.now()` and locale/timezone formatting
- Removes randomness from IDs, avatar colors, relative timestamps ("3 min ago" → fixed)
- Pins fonts (bundled with app, not OS-dependent) for cross-platform screenshot stability
- Uses fixed viewport sizes and device scale factors

**Test harness API** (available in fixture mode only):

```typescript
// Set complex UI states instantly without slow click-paths
window.__SCRIPTUM_TEST__ = {
  loadFixture(name: string): void           // Load a named fixture (e.g., 'overlap-warning')
  setDocContent(markdown: string): void     // Set document content directly
  setCursor(pos: {line: number, ch: number}): void
  spawnRemotePeer(peer: {name: string, type: 'human'|'agent', cursor: {line: number, ch: number}, section?: string}): void
  setGitStatus(status: {dirty: boolean, ahead: number, behind: number, lastCommit?: string}): void
  setSyncState(state: 'synced'|'offline'|'reconnecting'|'error'): void
  setCommentThreads(threads: CommentThread[]): void
}
```

This is the difference between tests that take 8 minutes and flake, and tests that take 30-90 seconds and are stable enough for agents to rely on.

**High-risk UI surfaces requiring golden baselines:**

1. **CodeMirror hybrid live preview**: Fixtures with representative markdown (headings, lists, tables, links, code, task lists). Assert "active line shows raw markdown" vs "inactive line renders rich" via visual baseline + DOM assertion. Stabilization: disable caret blink, disable smooth scroll, pin monospace font, fix editor width and wrap mode.
2. **Presence + section overlap overlays**: Spawn remote peers with deterministic colors/positions. Assert cursor labels render correctly, presence list shows correct names/types, section overlap severity changes (info vs warning), attribution badges render correctly (agent vs human).
3. **Sync/offline state indicators**: Deterministic sync states (offline banner, reconnecting, synced indicator, conflict/overlap warnings, git commit pending/last commit display).

**Tauri desktop testing strategy:**

- **Visual correctness**: Proved on the web build with Playwright (Chromium). Tauri on macOS uses WKWebView which has no WebDriver support — do not attempt macOS desktop visual tests.
- **Desktop-specific correctness**: WebDriver tests on Windows/Linux for Tauri-only features: file dialogs, daemon IPC wiring, file watcher integration, window/menu shortcuts.

**Agent-friendly test commands:**

| Command | Scope | Target |
|---------|-------|--------|
| `pnpm test:ui:smoke` | 10-20 key fixtures + ARIA snapshots (Chromium only) | < 2 min |
| `pnpm test:ui` | Full Playwright suite (Chromium + optional WebKit) | < 10 min |
| `pnpm test:ui:update` | Update baselines (only when explicitly invoked) | — |

Playwright configured to retain on failure: trace, screenshot diffs, video (optional). Agents get exact visual diffs to iterate against.

---

## Deployment Strategy

### Versioning
- Semver per component (daemon, CLI, desktop, relay, MCP server).
- Protocol compatibility: N and N-1 versions supported simultaneously.

### Hosted Relay
1. Feature flags gate new functionality.
2. Additive database migration applied first.
3. 5% canary for 30 minutes.
4. 50% rollout for 60 minutes.
5. 100% rollout.
6. Auto-rollback triggers: crash rate +2x baseline OR error rate > 1%.

### Desktop / CLI
- Ring deployment: internal -> beta -> GA.
- Kill-switch on crash regression (>2x baseline crash rate).
- Auto-update with user consent.

### Database Migrations
- Expand-only migrations (add columns, tables, indexes).
- Contract (drop old columns/tables) only in a later release after all clients have upgraded.
- Every migration includes pre/post validation checks and a rollback script.

---

## Migration Plan

### Import (New Workspace from Existing Files)
1. Scan `.md` files recursively.
2. Normalize UTF-8 (handle BOM), preserve line endings.
3. Initialize CRDT at `seq=0` for each document.
4. Build indexes (backlinks, tags, FTS).
5. Detect path collisions via `path_norm` uniqueness.

### Schema Migrations
- **Pattern**: Expand -> Backfill -> Switch -> Contract.
- Each phase has pre-checks (data integrity) and post-checks (no regression).
- Rollback script tested before applying forward migration.

### Protocol Versioning
- Client sends protocol version on connect (`scriptum-sync.v1`).
- Server rejects unsupported versions with `UPGRADE_REQUIRED` error.
- N-1 support maintained for at least one release cycle.

### Validation
- Post-migration verification: document counts, path uniqueness, sequence continuity.
- Automated comparison of pre/post migration state checksums.

---

# Part 6: Planning

## Development Phases

### Phase 0: Integration Spike (Week 0-1)
- [ ] **Critical path validation**: CodeMirror (JS) edits a doc via y-websocket connecting to daemon's local WS server. CLI (Rust) edits the same doc via y-crdt. Both converge. Persistence works across daemon restarts. This spike de-risks the Yjs-in-daemon architecture before building anything else.

### Phase 1: Foundation (Weeks 1-4)
- [ ] Daemon with local WebSocket server (Yjs-in-daemon model)
- [ ] Tauri app scaffold with React + CodeMirror 6 editor connecting to daemon via ws://localhost
- [ ] Build custom Live Preview CM6 extension (hybrid rendering: active line raw markdown, unfocused lines rich text). Reference: codemirror-markdown-hybrid (MIT)
- [ ] Yjs integration with CodeMirror 6 via y-codemirror.next → y-websocket → daemon
- [ ] File watcher pipeline (patch-based diff-to-Yjs with always-write-to-disk policy)
- [ ] Golden file tests for diff-to-Yjs edge cases
- [ ] Fixture mode + test harness API (`__SCRIPTUM_TEST__`) — build this into the scaffold early so UI testing is tight from day one
- [ ] Playwright smoke suite: 5-10 golden screens (editor core, sidebar, empty states) + ARIA snapshots
- [ ] Basic workspace management (create, open, list documents)
- [ ] Markdown rendering + editing (GFM support)

### Phase 2: Collaboration (Weeks 5-8)
- [ ] WebSocket relay server (Rust/Axum)
- [ ] y-websocket provider for document sync (daemon ↔ relay)
- [ ] WebRTC provider (y-webrtc) for P2P optimization (relay is primary)
- [ ] Presence/awareness (live cursors, online indicators)
- [ ] Section awareness layer
- [ ] Intent/lease system (advisory claims, TTL, overlap warnings)
- [ ] Reconciliation UI (concurrent rewrite detection → side-by-side view)
- [ ] Dual attribution model (CRDT-level origin tags + server-side update log)
- [ ] Basic web app (first-class: standalone editing without desktop, relay-backed CRDT state)

### Phase 3: Git & AI (Weeks 9-12)
- [ ] Git sync engine (gitoxide or libgit2)
- [ ] Git leader election protocol (relay-mediated, distributed among daemons)
- [ ] AI commit message generation (Claude API)
- [ ] Semantic commit groups (lease release, comment resolve, checkpoint as primary triggers; idle timer as fallback)
- [ ] `scriptum blame` command (CRDT-based per-line/section attribution)
- [ ] Git history browsing in the UI

### Phase 4: Agent Integration (Weeks 13-16)
- [ ] `scriptum` CLI tool (Rust) - connects to daemon via local WS / Unix socket
- [ ] CLI commands: `read`, `edit`, `tree`, `sections`, `search`, `diff`, `ls`, `blame`, `claim`, `bundle`
- [ ] Agent state commands: `whoami`, `status`, `conflicts`, `agents` (inspired by Niwa)
- [ ] Agent state persistence in daemon (survives context switches; duplicate names share state)
- [ ] MCP server for Claude Code / Cursor (TypeScript, stdio transport)
- [ ] MCP tools: `scriptum_read`, `scriptum_edit`, `scriptum_tree`, `scriptum_status`, `scriptum_conflicts`, `scriptum_claim`, `scriptum_bundle`
- [ ] Context bundling: `scriptum_bundle(doc, section, include=[...], token_budget=N)`
- [ ] Claude Code hooks: SessionStart, PreCompact, PreToolUse, PostToolUse, Stop (ported from Niwa)
- [ ] `scriptum setup claude` command to install hooks
- [ ] Agent attribution in UI (name badges, contribution indicators)

### Phase 5: Polish & Launch (Weeks 17-20)
- [ ] Permissions & sharing
- [ ] Search (full-text across documents — choose FTS5 vs Tantivy)
- [ ] Tags, backlinks (parsing, resolution, update-on-edit), wiki-style linking
- [ ] Commenting / inline threads
- [ ] Character-level time travel UI (scrub/replay, powered by dual attribution + WAL)
- [ ] Documentation & onboarding
- [ ] Performance optimization & load testing
- [ ] Property-based diff-to-Yjs tests (nightly)

---

## Open Questions

1. **Large documents**: Yjs performance degrades with very large documents (>1MB). Strategy: chunked updates vs sharding? Document size limit? Auto-splitting into sections? Lazy loading of CRDT state?

2. **CRDT tombstone compaction**: Yjs accumulates tombstones. Strategy for periodic garbage collection without breaking sync with long-offline peers?

3. ~~**Character-level attribution**~~ **RESOLVED**: Dual persistence model (CRDT-level origin tags + server-side update log). Retention: 90 days for update log, snapshots every 1000 updates.

4. **Mobile**: No mobile app in V1. When we add it (V2), daemon is embedded in-process (same as Tauri desktop mode) with local WS server. React Native vs native TBD.

5. **Tenant-level retention**: Custom retention policies and legal hold support for enterprise/hosted deployments.

6. **Pricing model**: Open-source relay server + hosted option. Per-user? Per-workspace? Free tier?

7. **Full-text search index**: SQLite FTS5 vs Tantivy. Needs decision before Phase 5.

8. **Backlinks parsing/resolution**: Table exists but no parsing, link resolution, or update-on-edit logic. Needs design before Phase 5.

9. **Git leader election protocol**: Relay-mediated leader election among daemons is decided, but the specific protocol (heartbeat-based, lease-based, Raft-lite) needs design before Phase 3.

10. **Reconciliation UI trigger heuristics**: When does "concurrent large rewrite" detection fire? Threshold: % of section changed by 2+ editors within N seconds? Needs UX research and tuning.

11. **E2EE key management (V2)**: Key rotation, device authorization, recovery flows for end-to-end encrypted workspaces. Deferred but storage layer should be designed to accommodate.
