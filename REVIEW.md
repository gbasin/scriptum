# Scriptum - Spec & Architecture Review

## Critical Issues

### 1. ~~The Y.Text vs Y.XmlFragment Problem~~ RESOLVED

**Decision: Y.Text with raw markdown.** CodeMirror 6 + y-codemirror.next works natively with Y.Text. No bridge to Y.XmlFragment needed. File watcher sync is simple text diffing via diff-match-patch.

### 2. ~~Tiptap Licensing~~ RESOLVED

**Decision: CodeMirror 6 (MIT).** Custom Live Preview extension built in TypeScript, referencing codemirror-markdown-hybrid architecture. No licensing concerns.

### 3. ~~Markdown Round-Trip Fidelity~~ RESOLVED

**Decision: Preserve exact formatting.** Y.Text stores raw markdown bytes. CodeMirror 6's live preview renders unfocused lines but never alters the stored text. No normalization, no phantom diffs.

### 4. File Watcher Race Conditions (Open - Before Phase 2)

Multiple failure modes need design:
- User saves from Vim at the exact moment Scriptum writes a remote update → data corruption
- VS Code auto-save + remote CRDT update arriving simultaneously → duplicate operations
- Multiple editors open on the same file (VS Code + Scriptum + Vim) → chaos
- Need: atomic write pattern (write-temp-then-rename), watcher pause during writes, event dedup
- Windows support (no fsevents/inotify, editors that lock files)

### 5. Auth Model Has No Real Design (Open - Before Phase 2)

The spec promises Owner/Editor/Viewer roles, document-level overrides, share links, and guest access. The architecture has: "JWT tokens" and "workspace isolation." Missing:
- Desktop app auth flow (OAuth device flow? Token?)
- Agent auth (CLI connects to daemon... but how does daemon authenticate to relay?)
- **CRDT-level access control**: Yjs has none. A "Viewer" connected via WebRTC can inject arbitrary operations. The relay can enforce permissions, but P2P peers can't.
- Guest access mechanism (anonymous Yjs peer? temporary token?)
- No RBAC schema, no permissions table

---

## Contradictions

### Spec vs Spec — All Fixed

| # | Original Contradiction | Resolution |
|---|----------------------|------------|
| 1 | "Local-first" vs "Cloud-only" workspace option | Removed cloud-only option. Local files always present. |
| 2 | "Not a wiki" vs backlinks | Clarified: backlinks are navigation, not a wiki system. |
| 3 | "Not Google Docs" vs comments resolve/unresolve | Clarified: comments are discussion, not approval workflow. |
| 4 | Idle-triggered vs periodic auto-commit | Standardized on idle-triggered (~30s inactivity). |
| 5 | "Full GFM" including non-GFM extensions | Changed to "GFM core + extensions (KaTeX, Mermaid)". |

### Spec vs Architecture — All Fixed

| # | Original Contradiction | Resolution |
|---|----------------------|------------|
| 6 | Spec listed 6 MCP tools, Architecture 9 | Spec updated to list all 9 tools. |
| 7 | Spec listed 2 MCP resources, Architecture 4 | Spec updated to list all 4 resources. |
| 8 | Phase 4 MCP tools didn't match agent section | Aligned in architecture. |
| 9 | Haiku vs Sonnet for commit messages | Standardized on Haiku (cheap, fast). Code example updated. |

### Architecture Internal — All Fixed

| # | Original Contradiction | Resolution |
|---|----------------------|------------|
| 10 | Relay labeled "Fallback" but actually required | Relabeled "Required (sync + persistence)". Relay connects first. |
| 11 | JSON files vs SQLite for relay metadata | Consolidated to single SQLite database (relay.db). |
| 12 | File watcher: Rust or TypeScript? | Clarified: runs in daemon (Rust). TypeScript code is pseudocode. |
| 13 | MCP (TypeScript) ↔ daemon (Rust) IPC unspecified | Specified: JSON-RPC over Unix socket / named pipe. |

---

## ~~Daemon Architecture Gap~~ RESOLVED

**Decision: Hybrid daemon.**
- Standalone Rust process, single instance per machine
- When Tauri desktop is running: daemon embedded in Tauri's Rust backend (in-process)
- When CLI/MCP used without desktop: daemon runs as standalone background process (auto-started)
- Owns: Yjs docs, file watcher, section awareness, agent state, git sync scheduling
- IPC: Unix socket (macOS/Linux), named pipe (Windows)
- Protocol: JSON-RPC
- Crash recovery: Yjs state persisted to .yjs files, daemon restarts and reloads

---

## Features With Zero Architectural Support

These are promised in the spec but have no data model, schema, or technical approach:

| Feature | What's Missing | Priority |
|---------|---------------|----------|
| **Commenting** | No CRDT structure, no schema, no anchor tracking, no threading model. Comments can't survive markdown round-trips (markdown has no comment syntax). | Defer to V2 |
| **Permissions/RBAC** | No role definitions, no permissions table, no enforcement mechanism | Defer to V2 (single-role V1) |
| **Full-text search** | No search index (SQLite FTS? Tantivy? MeiliSearch?) | Before Phase 5 |
| **Image/file upload** | No blob storage, no CDN, no upload endpoint, no binary handling | Defer to V2 (paste URLs for V1) |
| **KaTeX/Mermaid** | No extensions listed, no rendering architecture | CM6 extensions, client-side rendering |
| **Timeline view** | No mechanism to reconstruct document at arbitrary point in time | Defer to V2 (git history for V1) |
| **Diff view** | No diff computation between arbitrary versions | Defer to V2 (git diff for V1) |
| **Tags syncing** | Tags table exists locally but no sync mechanism to relay | Before Phase 5 |
| **Backlinks** | Table exists but no parsing, resolution, or update logic | Before Phase 5 |
| **30-day CRDT history** | No retention policy, no GC strategy, Yjs files are append-only | Before Phase 5 |

---

## UX Gaps by User Type

### Non-Technical PM (Web)
- No signup flow for non-GitHub users (email/password path is "optional" and unspecified)
- No invitation flow (how do they get access?)
- No onboarding or empty-state guidance
- No notification system at all (edits, comments, mentions)
- Section IDs like "h2_3" would leak to UI - need friendly names
- CRDT interleaving will feel like a bug (no "accept/reject" affordance)
- No settings UI (must edit TOML files?)
- Comment interaction model unspecified (@mentions? notification? panel?)

### Developer (Desktop/CLI)
- No install instructions (dmg? brew? cargo install?)
- ~~Desktop app vs CLI vs daemon topology is confusing~~ Resolved: hybrid daemon documented
- No `scriptum init` command for workspace creation
- Connecting to existing git repo is unspecified
- "Yjs is the source of truth, not the file" is counterintuitive - what happens when they `git pull`?
- Auto-commit every 30s will pollute git history (no squash/batch option)
- No team invitation flow

### AI Agent (CLI/MCP)
- ~~MCP server ↔ daemon communication path unspecified~~ Resolved: JSON-RPC over Unix socket
- Agent name collisions: `whoami` suggests but doesn't enforce unique names
- No agent coordination/reservation system ("I'm working on this section")
- Two Claude instances with same name will corrupt state
- Post-compaction: agent knows its name and section but not the content → needs re-read
- MCP `scriptum_subscribe` notification delivery over stdio is unclear

### Returning Offline User
- No sync progress indicator (could be thousands of ops after 3 days)
- No "here's what changed while you were away" summary
- CRDT GC may have run during offline period → sync fails with no recovery path
- Section IDs may have shifted → confusing attribution

### Large Team (10+ editors)
- 10 live cursors with name labels = visual chaos. No density management.
- WebRTC mesh: 10 peers = 45 connections. Need auto-switch to relay at ~4-5 peers.
- Section re-parse on every CRDT update from 10 typists = potential lag
- Overlap warnings become noise with 10 editors

### Desktop <> Web Switching
- ~~Relay may not have latest state if desktop used P2P only~~ Resolved: relay always connected first
- Cursor/presence doesn't transfer between clients
- No cross-device identity linking (desktop local identity vs web GitHub OAuth)
- No "continue on web" handoff

### Cross-Cutting
- No error states described anywhere (relay down? git push fail? API unavailable?)
- No loading states (syncing? committing? loading?)
- No document creation or deletion flow
- No accessibility (keyboard nav, screen readers, ARIA)
- No responsive web design (PM on mobile?)
- Onboarding is Phase 5 (weeks 17-20) but "< 2 min to first edit" is a success metric

---

## Remaining Priority for Resolution

### Before Phase 2 (blocks collaboration)
1. **Auth model design** → at minimum: how desktop/CLI/agent authenticate to relay
2. **File watcher race condition strategy** → atomic writes, watcher pause, event dedup
3. **Relay always-connected enforcement** → architecture updated, needs implementation design

### Before Phase 4 (blocks agent integration)
4. **Agent name uniqueness enforcement** → daemon-level dedup or registry
5. **MCP subscribe notification delivery** → how stdio transport delivers push notifications

### Before Phase 5 (blocks polish)
6. **Full-text search index** → SQLite FTS5 vs Tantivy
7. **Tags/backlinks sync to relay** → extend relay protocol
8. **CRDT GC / retention policy** → Yjs tombstone management

### Defer to V2
- Commenting (needs its own design doc)
- Permissions/RBAC (single-role for V1)
- Image upload (paste URLs for V1)
- Timeline/diff views (git history for V1)
- Mobile/responsive
- Notification system
