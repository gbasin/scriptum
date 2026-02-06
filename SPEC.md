# Scriptum - Product Specification

> Local-first collaborative markdown with seamless git sync and first-class agent support.

## Vision

Writing should feel like writing, not committing. Collaboration should be effortless. Your files are yours, locally.

Scriptum bridges the gap between GitHub (too heavy for collaboration) and Notion (too locked-in, hostile to local editing). It's a **hosted markdown collaboration tool** that lets you edit files locally with any editor, collaborate in real-time on the web, and automatically syncs to git with AI-generated commit messages.

## Core Principles

1. **Local-first**: Your data lives on your machine. Works offline. Syncs when connected.
2. **Markdown-native**: Pure `.md` files on disk. No proprietary format. No lock-in.
3. **CRDT-native**: Conflict-free by design. Multiple editors, humans or agents, never conflict destructively.
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

A hybrid live-preview editing experience powered by **CodeMirror 6** with a custom live preview extension (reference: `codemirror-markdown-hybrid`, MIT). The active line shows raw markdown for precise editing; unfocused lines render as rich text (similar to Obsidian's live preview). This preserves exact markdown formatting — what you type is what gets stored.

- **Hybrid live preview**: Active line shows raw markdown, unfocused lines render inline as rich text (headings, bold, links, etc.)
- **Exact formatting preservation**: No normalization. Raw markdown is stored as-is in the CRDT and on disk.
- **Pure markdown storage**: Files on disk are always valid `.md` files — the exact bytes you typed
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
- **Commenting**: Inline comments on selections, threaded discussions, resolve/unresolve. Comments are for discussion and conversation — not an approval workflow. No tracked changes or suggesting mode.

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
- Tools: `scriptum_read`, `scriptum_edit`, `scriptum_list`, `scriptum_tree`, `scriptum_status`, `scriptum_conflicts`, `scriptum_history`, `scriptum_subscribe`, `scriptum_agents`
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

**Agent Attribution**:
- All agent edits are tracked with agent name, timestamp, and edit summary
- Version history shows human vs. agent contributions
- Section-level "last edited by" indicators in the UI
- Contributions flow through to git via `Co-authored-by` headers

### 5. Git Sync

Automatic, intelligent syncing to git remotes.

- **Auto-commit on idle**: After ~30 seconds of inactivity (no new edits), changes are committed automatically
- **AI-generated commit messages**: Analyzes the diff and produces meaningful commit messages (e.g., "Update authentication flow with OAuth2 PKCE details")
- **Configurable remote**: GitHub, GitLab, Bitbucket, any git remote
- **Branch strategy**: Configurable per workspace - commit to main, to a branch, or create PRs
- **Selective sync**: Choose which documents/folders sync to git
- **Git blame integration**: Attribution flows through to git - `git blame` shows who (human or agent) wrote each line

### 6. Workspace Organization

Abstract workspace layer that's flexible and intuitive.

- **Workspaces**: Top-level containers for related documents
- **Folders**: Hierarchical organization within workspaces
- **Tags**: Cross-cutting labels for documents (e.g., `#rfc`, `#draft`, `#approved`)
- **Backlinks**: `[[wiki-style]]` links between documents for navigation convenience. This is a linking/navigation feature, not a wiki system — no namespaces, templates, or wiki-specific features.
- **Search**: Full-text search across all documents, with filters by tag, author, date
- **Flexible backends**: A workspace can be backed by:
  - A git repo (full sync)
  - A local folder (file system only)
  - Local files are always present — the relay server is for sync and collaboration, not primary storage

### 7. Version History & Attribution

Full audit trail of every change.

- **CRDT history (recent)**: Character-level, real-time history for the last 30 days. See exactly who typed what, rewind to any point.
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
- **Not a CMS**: No publishing pipeline, SEO, or public-facing rendering
- **Not Notion**: No databases, kanban boards, or non-document content types
- **Not Google Docs**: No suggesting mode, tracked changes, or approval workflows. Comments exist for discussion, not as an approval primitive.

---

## Success Metrics

- **Time to first collaborative edit**: < 2 minutes from install
- **Sync latency**: < 500ms for CRDT updates between peers
- **Git commit quality**: AI commit messages rated "good" by users >80% of the time
- **Offline resilience**: 100% of offline edits merge without data loss
- **Agent integration**: Claude Code can edit a Scriptum doc with < 5 lines of config
