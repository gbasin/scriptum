# Contributing to Scriptum

Thanks for contributing to Scriptum. This guide covers setup, standards, testing, and pull request expectations for this monorepo.

## 1. Development Setup

### Prerequisites

- Node.js `20+`
- pnpm `10+`
- Rust stable toolchain (Rust 2021 compatible)
- Optional but recommended: `cargo-nextest` and `cargo-llvm-cov`

Install tools:

```bash
rustup default stable
cargo install cargo-nextest
cargo install cargo-llvm-cov
```

Clone and install dependencies:

```bash
git clone https://github.com/garybasin/scriptum.git
cd scriptum
pnpm install
```

Build and run baseline checks:

```bash
pnpm build
pnpm ci:precommit
```

Run the local dev stack:

```bash
pnpm dev
```

## 2. Project Structure

Scriptum is a monorepo with Rust workspace crates and TypeScript packages.

- `crates/common`: shared Rust CRDT/parsing/protocol primitives
- `crates/daemon`: `scriptumd` daemon (authoritative local state, file watcher, local WS)
- `crates/cli`: `scriptum` CLI
- `crates/relay`: optional relay service for multi-user collaboration
- `packages/editor`: shared CodeMirror extensions
- `packages/web`: web app
- `packages/desktop`: Tauri desktop shell
- `packages/shared`: shared TS types/protocol/API client
- `packages/mcp-server`: MCP server package

## 3. Coding Standards

### Rust

- Keep code `rustfmt` formatted: `cargo fmt --all`
- Keep clippy clean: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- Prefer `cargo nextest run --workspace` when available; otherwise `cargo test --workspace`
- Favor clear, small modules and root-cause fixes over ad hoc patches

### TypeScript

- Run Biome checks: `pnpm check` (or package-scoped checks as needed)
- Keep tests passing with Vitest
- Keep typechecks green with `tsc --noEmit` (via package `typecheck` scripts)
- Avoid large cross-package refactors in a single PR unless required by the task

## 4. Testing Expectations

- Add or update unit tests for all new behavior
- Add golden/regression tests for diff/CRDT edge cases when behavior is stateful or algorithmic
- Add integration tests for cross-component flows (CLI↔daemon, daemon↔relay, etc.) when behavior spans boundaries
- Add Playwright E2E coverage for user-visible UI changes
- Run tests scoped to changed areas during development; run `pnpm ci:precommit` before opening/merging

## 5. PR Process

- Use branch names by change type:
  - `feat/<short-topic>`
  - `fix/<short-topic>`
  - `docs/<short-topic>`
- Use Conventional Commit messages (`feat:`, `fix:`, `docs:`, `refactor:`)
- Keep commits small and focused
- Include clear PR descriptions:
  - problem statement
  - implementation summary
  - test evidence
  - follow-up items (if any)
- CI must pass before merge

## 6. Architecture Notes

- Scriptum is CRDT-first. Collaboration and conflict handling are built around Yjs/y-crdt convergence.
- The daemon is the source of truth for local document state and sync orchestration.
- The relay is an optional collaboration service; local-first behavior should continue to work without it.
- APIs are designed for both humans and agents. Preserve explicit contracts, stable method names, and attribution flows.

If you are unsure about a behavior, start by reading `SPEC.md` and relevant tests, then propose the smallest change that keeps architecture and contracts coherent.
