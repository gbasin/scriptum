# Scriptum - Spec & Architecture Review

## Critical Issues

### 1. ~~The Y.Text vs Y.XmlFragment Problem~~ RESOLVED

**Decision: Y.Text with raw markdown.** CodeMirror 6 + y-codemirror.next works natively with Y.Text. No bridge to Y.XmlFragment needed. File watcher sync is simple text diffing via diff-match-patch.

### 2. ~~Tiptap Licensing~~ RESOLVED

**Decision: CodeMirror 6 (MIT).** Custom Live Preview extension built in TypeScript, referencing codemirror-markdown-hybrid architecture. No licensing concerns.

### 3. ~~Markdown Round-Trip Fidelity~~ RESOLVED

**Decision: Preserve exact formatting.** Y.Text stores raw markdown bytes. CodeMirror 6's live preview renders unfocused lines but never alters the stored text. No normalization, no phantom diffs.

### 4. ~~File Watcher Race Conditions~~ RESOLVED

Detailed File Watcher Pipeline now in SPEC.md Part 2, Section 4. Covers:
- Atomic write pattern (write-temp-then-rename)
- Watcher pause during daemon writes
- Event dedup via content hash
- diff-match-patch for external change → CRDT integration
- Cross-platform: fsevents (macOS), inotify (Linux), ReadDirectoryChanges (Windows)

### 5. ~~Auth Model~~ RESOLVED

Full auth design now in SPEC.md:
- **Auth flow**: OAuth 2.1 + PKCE (GitHub as initial provider), optional email/password (Argon2id)
- **Desktop auth**: OAuth device flow or browser-redirect via Tauri deep links
- **Agent auth**: CLI/MCP connect to local daemon; daemon authenticates to relay with stored credentials
- **Tokens**: 15-min access JWT (workspace-scoped), 30-day rotating refresh tokens with reuse detection
- **RBAC**: Owner/Editor/Viewer at workspace level, document ACL overrides with optional expiry
- **Share links**: 128-bit random token (stored hashed), optional password, max uses, expiry
- **DB schema**: Full PostgreSQL tables for users, refresh_sessions, workspace_members, document_acl_overrides, share_links
- **CRDT access control**: Relay enforces permissions; P2P peers cannot bypass (relay is required)

---

## Contradictions

### All Fixed

All contradictions identified in the original review and the subsequent AI critique have been resolved:
- **Local-first vs relay required**: Clarified — local replica is authoritative, relay for collab only.
- **Yjs location (JS vs Rust daemon)**: Resolved — Yjs-in-daemon with local WS server. CodeMirror connects via y-websocket.
- **"Byte-for-byte" claim**: Adjusted to "exact content, no normalization, UTF-8."
- **"Never conflict destructively"**: Reframed to "Conflict-free data, coordinated editing" with intent/lease system and reconciliation UI.
- **Git blame attribution**: Two-tier model — `scriptum blame` (CRDT-based) + git co-author trailers.
- **Web app vs "local files always present"**: Web is first-class (relay-backed). "Local files" scoped to desktop.
- **Disk write policy**: Always write to disk. External editors handle "file changed" natively.
- **Attribution persistence**: Dual model — CRDT origin tags + server-side update log.
- **Git sync multi-writer**: Leader election among daemons, relay-mediated.
- **Encryption model**: Server-readable V1, E2EE opt-in V2.

---

## ~~Daemon Architecture Gap~~ RESOLVED

**Decision: Hybrid daemon.**
- Standalone Rust process, single instance per machine
- When Tauri desktop is running: daemon embedded in Tauri's Rust backend (in-process)
- When CLI/MCP used without desktop: daemon runs as standalone background process (auto-started)
- Owns: Yjs docs, file watcher, section awareness, agent state, git sync scheduling
- IPC: Unix socket (macOS/Linux), named pipe (Windows)
- Protocol: JSON-RPC
- Crash recovery: Yjs state persisted to WAL + snapshots, daemon restarts and reloads

---

## Features — Architectural Support Status

After the adversarial review with GPT-5.3-codex, the spec now covers most features that previously had zero architectural support:

| Feature | Status | Notes |
|---------|--------|-------|
| **Commenting** | Designed | REST endpoints, relay DB schema (comments, comment_messages tables), anchor tracking via section_id + char offsets. Threading model specified. Still deferred to V2 for implementation. |
| **Permissions/RBAC** | Designed | Full RBAC (Owner/Editor/Viewer), workspace_members table, document_acl_overrides table, share_links table. Enforcement at relay. |
| **Full-text search** | Open | No search index chosen yet (SQLite FTS5 vs Tantivy). Needed before Phase 5. |
| **Image/file upload** | Deferred | No blob storage. Paste URLs for V1. |
| **KaTeX/Mermaid** | Deferred | CM6 extensions, client-side rendering. No detailed design yet. |
| **Timeline view** | Deferred | Character-level time travel UI is V1 (Phase 5). |
| **Diff view** | Deferred | Git diff for V1. |
| **Tags syncing** | Designed | Tags table in relay DB schema. Sync via relay protocol. |
| **Backlinks** | Open | Table exists locally but no parsing, resolution, or update logic. Before Phase 5. |
| **CRDT history/retention** | Designed | 90-day retention, WAL + snapshots, dual attribution model, character-level replay committed for V1. |
| **Intent/lease system** | Designed | Advisory leases, TTL, overlap warnings. V1 Phase 2. |
| **Reconciliation UI** | Designed | Concurrent rewrite detection → side-by-side view. V1 Phase 2. |
| **Semantic commit groups** | Designed | Intent-based triggers (lease release, comment resolve, checkpoint). Idle timer as fallback. V1 Phase 3. |
| **Context bundling** | Designed | `scriptum_bundle` with token budget. V1 Phase 4. |
| **Error handling** | Designed | Full error code registry (18 codes), HTTP status mapping, retry policy, dead letter queue, deterministic fallbacks. |
| **Observability** | Designed | Logging, metrics (15+ metrics defined), distributed tracing, alerting thresholds. |
| **Performance SLAs** | Designed | 12 metrics with specific targets and benchmark assumptions. |
| **Deployment** | Designed | Versioning strategy, hosted relay (Docker/K8s), desktop/CLI distribution, DB migration approach. |

---

## UX Gaps by User Type

### Non-Technical PM (Web)
- No signup flow for non-GitHub users (email/password path is "optional" and unspecified beyond Argon2id)
- No invitation flow (workspace_members table exists but no invite UI/email flow)
- No onboarding or empty-state guidance
- No notification system at all (edits, comments, mentions)
- Section IDs like "h2_3" would leak to UI - need friendly names
- ~~CRDT interleaving will feel like a bug~~ **RESOLVED**: Reconciliation UI shows side-by-side when concurrent large rewrites detected
- No settings UI (must edit TOML files?)

### Developer (Desktop/CLI)
- No install instructions (dmg? brew? cargo install?)
- No `scriptum init` command for workspace creation
- Connecting to existing git repo is unspecified
- "Yjs is the source of truth, not the file" is counterintuitive - what happens when they `git pull`?
- ~~Auto-commit every 30s will pollute git history~~ **RESOLVED**: Semantic commit groups (intent-based triggers), idle timer is fallback only

### AI Agent (CLI/MCP)
- ~~Agent name collisions~~ **RESOLVED**: Duplicates allowed, shared state by design
- ~~No agent coordination/reservation system~~ **RESOLVED**: Intent/lease system with `scriptum_claim`
- ~~Two Claude instances with same name will corrupt state~~ **RESOLVED**: Shared state, not corruption
- Post-compaction: agent knows its name and section but not the content → needs re-read (mitigated by `scriptum_bundle`)
- MCP `scriptum_subscribe` notification delivery over stdio is unclear

### Returning Offline User
- No sync progress indicator (could be thousands of ops after 3 days)
- No "here's what changed while you were away" summary
- Section IDs may have shifted → confusing attribution

### Large Team (10+ editors)
- 10 live cursors with name labels = visual chaos. No density management.
- WebRTC mesh: 10 peers = 45 connections. Need auto-switch to relay at ~4-5 peers.
- Section re-parse on every CRDT update from 10 typists = potential lag
- Overlap warnings become noise with 10 editors

### Desktop <> Web Switching
- Cursor/presence doesn't transfer between clients
- No cross-device identity linking (desktop local identity vs web GitHub OAuth)
- No "continue on web" handoff

### Cross-Cutting
- ~~No error states described anywhere~~ Resolved: full error code registry in spec
- ~~No loading states~~ **RESOLVED**: Sync status in status bar (green/yellow/red), offline banner on web, reconnect progress indicator
- ~~No document creation or deletion flow~~ **RESOLVED**: UI/UX section defines Cmd+N, right-click context menus, sidebar actions
- ~~No accessibility (keyboard nav, screen readers, ARIA)~~ Partially addressed: ARIA snapshot testing added to UI testing strategy. Keyboard nav and screen reader design still needed.
- No responsive web design (PM on mobile?)
- Onboarding is Phase 5 (weeks 17-20) but "< 2 min to first edit" is a success metric

---

## Remaining Priority for Resolution

### Before Phase 0 (blocks everything)
1. ~~Integration spike~~ → Added as Phase 0 in spec. Must validate Yjs-in-daemon + CodeMirror + CLI convergence.

### Before Phase 2 (blocks collaboration)
2. ~~Auth model design~~ **RESOLVED**
3. ~~File watcher race condition strategy~~ **RESOLVED**
4. ~~Relay always-connected enforcement~~ **RESOLVED** — relay for collab, local-first for solo/offline
5. ~~CRDT location model~~ **RESOLVED** — Yjs-in-daemon with local WS server
6. ~~Disk write policy~~ **RESOLVED** — always write to disk
7. ~~Conflict framing~~ **RESOLVED** — "Conflict-free data, coordinated editing" + intent/leases + reconciliation UI

### Before Phase 3 (blocks git sync)
8. ~~Git leader election protocol~~ **RESOLVED** — lease-based via relay (TTL 60s, auto-renew, failover on expiry)

### Before Phase 4 (blocks agent integration)
9. ~~Agent name policy~~ **RESOLVED** — duplicates allowed, shared state
10. ~~MCP subscribe notification delivery~~ **RESOLVED** — polling with change token, no push over stdio

### Before Phase 5 (blocks polish)
11. ~~Full-text search index~~ **RESOLVED** — SQLite FTS5 for V1, Tantivy later if needed
12. ~~Backlinks parsing/resolution~~ **RESOLVED** — Obsidian-compatible syntax, resolve path→filename→title, index on save/commit, auto-update on rename
13. ~~CRDT GC / retention policy~~ **RESOLVED**
14. ~~Reconciliation UI trigger heuristics~~ **RESOLVED** — >50% section changed by 2+ editors in 30s, inline resolution

### Defer to V2
- Image upload (paste URLs for V1)
- E2EE opt-in (server-readable V1, storage layer designed to accommodate)
- Mobile (daemon embedded in-process, same as Tauri)
- Notification system
- KaTeX/Mermaid extensions
