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

## Quickstart

1. Install prerequisites:
   Follow [Prerequisites](#prerequisites) to install Rust, Node.js, and pnpm.
   Expected result: `cargo`, `rustc`, `node`, and `pnpm` are available in your shell.

2. Clone and install dependencies:

   ```bash
   git clone https://github.com/garybasin/scriptum.git
   cd scriptum
   pnpm install
   ```

   Expected result: workspace dependencies are installed without lockfile errors.

3. Build all packages:

   ```bash
   pnpm build
   ```

   Expected result: Rust crates and TypeScript packages compile successfully.

4. Initialize a workspace:

   ```bash
   scriptum init ~/my-notes
   ```

   Expected result: `~/my-notes/.scriptum/` is created and registered as a Scriptum workspace.

5. Start the development stack:

   ```bash
   pnpm dev
   ```

   Expected result: web (`:5173`), daemon (`scriptumd`), and relay logs appear in one terminal.

6. Open the app:
   Browse to `http://localhost:5173`.
   Expected result: Scriptum web UI loads and connects to the local daemon.

7. Create your first document:
   Create a new markdown document in the UI and start editing.
   Expected result: edits persist locally and are reflected in the active workspace.

What just happened?
`pnpm dev` runs the web app, daemon, and relay together. The web editor connects to `scriptumd` for local-first document state, while the relay enables multi-user sync flows.

### Quickstart Troubleshooting

- `scriptum` or `cargo` command not found:
  Re-open your terminal after installing rustup/CLI and verify `PATH` includes Cargo binaries (`~/.cargo/bin` on macOS/Linux).

- Daemon fails to start:
  Check for stale runtime files and remove them before retrying: `rm -f ~/.scriptum/daemon.sock ~/.scriptum/ws.port`.

- Port already in use (`5173` or relay port):
  Stop conflicting processes or override ports (for web: `pnpm --filter @scriptum/web dev -- --port 5174`).

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
pnpm ci:precommit         # Parallel pre-commit checks (lint + typecheck + all tests)
```

Rust coverage requires `cargo-llvm-cov`:

```bash
cargo install cargo-llvm-cov
```

### Relay (Docker)

```bash
cp docker/.env.example docker/.env
docker compose -f docker/compose.yml up --build
curl http://localhost:8080/healthz
```

### Relay OAuth Setup

For full GitHub OAuth app setup (including callback URL, required env vars, Docker `.env`, and direct `cargo run` flow), see:

- `docs/relay-setup.md`

## MCP Server

`@scriptum/mcp-server` ships a `scriptum-mcp` CLI for stdio MCP clients.

Prerequisite:
- Scriptum daemon must be running locally (`scriptumd`).
- Quick check: `scriptum status`.

Install globally:

```bash
npm install -g @scriptum/mcp-server
```

Or run without global install:

```bash
npx -y @scriptum/mcp-server
```

Local repo path (without npm publish):

```bash
pnpm --filter @scriptum/mcp-server run build
node packages/mcp-server/dist/index.js
```

Verify the MCP server entrypoint:

```bash
scriptum-mcp
```

`scriptum-mcp` should start and wait on stdio for MCP handshake input.

### Claude Code

Add to `.claude/settings.json`:

```json
{
  "mcp_servers": {
    "scriptum": {
      "command": "scriptum-mcp",
      "args": []
    }
  }
}
```

If your MCP client expects camelCase keys instead of snake_case, use `mcpServers`.

### Cursor

In Cursor, open MCP settings and add a stdio server:
- Name: `scriptum`
- Command: `scriptum-mcp`
- Args: none (or `["-y", "@scriptum/mcp-server"]` when using `npx`)

### Local Path Config

```json
{
  "mcp_servers": {
    "scriptum": {
      "command": "node",
      "args": ["/absolute/path/to/scriptum/packages/mcp-server/dist/index.js"]
    }
  }
}
```

### Available Tools

- `scriptum_read`
- `scriptum_edit`
- `scriptum_list`
- `scriptum_tree`
- `scriptum_status`
- `scriptum_conflicts`
- `scriptum_agents`
- `scriptum_claim`
- `scriptum_bundle`
- `scriptum_subscribe`
- `scriptum_history`

### Available Resources

- `scriptum://workspace`
- `scriptum://agents`
- `scriptum://docs/{id}`
- `scriptum://docs/{id}/sections`

Note: `scriptum://docs/{id}` and `scriptum://docs/{id}/sections` are resource templates resolved by document ID.

## License

TBD
