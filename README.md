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

Scriptum is in active implementation (not spec-only).

- Core code is implemented across Rust workspace crates (`common`, `daemon`, `cli`, `relay`) and TypeScript packages (`web`, `desktop`, `editor`, `mcp-server`, `shared`).
- CI, linting, unit tests, Playwright smoke coverage, and runbooks are part of the repository.
- Product work is ongoing; expect active iteration across collaboration, sync, auth, and desktop reliability.

See [SPEC.md](SPEC.md) for the full product and architecture reference.

## Prerequisites

- Node.js `20+`
- pnpm `10+`
- Rust toolchain via [rustup](https://rustup.rs/) (stable toolchain, Rust 2021 edition compatible)

Install Rust on macOS/Linux:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable
```

Install Rust test and coverage tools:

```bash
cargo install cargo-nextest
cargo install cargo-llvm-cov
```

## Starting the Daemon

Scriptum clients talk to the local daemon (`scriptumd`) over a Unix socket.

Start it directly:

```bash
cargo run --bin scriptumd
```

Or run the built binary:

```bash
./target/debug/scriptumd
```

Socket-activated mode:
- The `scriptum` CLI can auto-start the daemon when needed.
- This is useful for one-off CLI workflows where you do not want a dedicated daemon terminal.

Runtime files:
- Unix socket: `~/.scriptum/daemon.sock`
- WebSocket port file: `~/.scriptum/ws.port`

Verify daemon health:

```bash
scriptum status
```

## Development

```bash
pnpm dev                  # Start web + daemon + relay (parallel)
pnpm dev:local            # Start web + daemon in local-only mode (no relay)
pnpm dev:web              # Start only the web dev server
pnpm install              # Install dependencies
pnpm build                # Build all packages (Turborepo)
pnpm test                 # Run unit tests
pnpm coverage:ts          # Run TypeScript coverage (Vitest)
pnpm coverage:rust        # Run Rust coverage (cargo-llvm-cov)
pnpm coverage             # Run TS + Rust coverage
pnpm lint                 # Lint (Biome)
pnpm test:ui:smoke        # Playwright smoke tests
pnpm ci:fast              # Fast CI pipeline
```

Rust coverage requires `cargo-llvm-cov`:

```bash
cargo install cargo-llvm-cov
```

### Relay (Docker)

```bash
docker compose -f docker/compose.yml up --build
curl http://localhost:8080/healthz
```

## MCP Server

`@scriptum/mcp-server` ships a `scriptum-mcp` CLI for stdio MCP clients.

```bash
npx -y @scriptum/mcp-server
```

Claude Code MCP config:

```json
{
  "mcpServers": {
    "scriptum": {
      "command": "npx",
      "args": ["-y", "@scriptum/mcp-server"]
    }
  }
}
```

Local repo config (without npm publish):

```json
{
  "mcpServers": {
    "scriptum": {
      "command": "node",
      "args": ["/absolute/path/to/scriptum/packages/mcp-server/dist/index.js"]
    }
  }
}
```

## License

TBD
