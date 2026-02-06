# Scriptum

Local-first collaborative markdown with seamless git sync and first-class agent support.

## What is Scriptum?

Scriptum bridges the gap between GitHub (too heavy for collaboration) and Notion (too locked-in, hostile to local editing). Edit markdown files locally with any editor, collaborate in real-time on the web, and automatically sync to git with AI-generated commit messages.

### Core Principles

- **Local-first** — Your data lives on your machine. Works fully offline.
- **Markdown-native** — Pure `.md` files on disk. No proprietary format.
- **Conflict-free** — Yjs CRDT ensures no data loss. Intent system coordinates concurrent edits.
- **Git-optional** — Push to any git remote with AI-generated commits. Or don't.
- **Agent-friendly** — AI agents are first-class collaborators with attribution.

## Architecture

```
Desktop (Tauri) / Web (React) / CLI (Rust) / MCP Server (TypeScript)
                          │
                    Yjs CRDT Engine
                    (daemon, local WS)
                          │
          ┌───────────────┼───────────────┐
     File Watcher    Section Awareness   Git Sync
      (local FS)    (markdown parser)   (AI commits)
                          │
                     Relay Server
                   (Rust/Axum, optional)
```

- **Daemon** — Owns the authoritative Y.Doc, exposes local WebSocket for all clients
- **Relay** — Optional server for multi-user collaboration and cross-device sync
- **CLI** — `scriptum read`, `scriptum edit`, `scriptum blame`, etc.
- **MCP Server** — Native integration with Claude Code, Cursor, and MCP-compatible agents

## Tech Stack

| Component | Technology |
|-----------|-----------|
| Desktop | Tauri 2.0 + React 19 + CodeMirror 6 |
| Web | React 19 + CodeMirror 6 + Vite |
| CLI & Daemon | Rust (Cargo workspace) |
| Relay | Rust (Axum) + PostgreSQL |
| MCP Server | TypeScript (Node.js) |
| CRDT | Yjs / y-crdt |
| Styling | Tailwind CSS 4 |
| State | Zustand |
| Monorepo | Turborepo + pnpm + Cargo workspace |

## Project Structure

```
scriptum/
├── crates/
│   ├── common/          # Shared Rust: CRDT ops, section parser, protocol types
│   ├── daemon/          # scriptumd (Yjs engine, file watcher, WS server)
│   ├── cli/             # scriptum CLI
│   └── relay/           # Relay server (Axum)
├── packages/
│   ├── editor/          # Shared CodeMirror 6 extensions
│   ├── web/             # Web app (React)
│   ├── desktop/         # Tauri shell (wraps web + daemon)
│   ├── mcp-server/      # MCP server (TypeScript, stdio)
│   └── shared/          # Shared TS types, API client, protocol definitions
├── Cargo.toml           # Cargo workspace root
├── turbo.json           # Turborepo pipeline config
├── pnpm-workspace.yaml
└── package.json
```

## Status

Spec-only phase — no code yet. See [SPEC.md](SPEC.md) for the complete specification.

## Relay Docker Dev

Start local relay + PostgreSQL:

```bash
docker compose -f docker/compose.yml up --build
```

Relay health check:

```bash
curl http://localhost:8080/healthz
```

## License

TBD
