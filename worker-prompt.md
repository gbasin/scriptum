You are working on the **Scriptum** project — a local-first collaborative markdown tool with git sync and first-class agent support.

Read `AGENTS.md` for full project rules, beads workflow, git conventions, and session protocol. Everything below supplements — not replaces — those instructions.

## Your mission

1. Run `bv --robot-next` to get the single highest-impact task
2. **⚠️ CLAIM IT IMMEDIATELY — before reading code, planning, or doing anything else:**
   `br update <id> --claim`
   `--claim` is atomic: it sets assignee + status=in_progress and **fails if another agent already claimed it**. If it fails, run `bv --robot-next` again for a different task. DO NOT PROCEED until the claim succeeds.
3. Read the SPEC.md for implementation details relevant to your task (use grep/search — it's 3000+ lines)
4. Implement the task, writing tests where appropriate
5. When done: `br close <id> --reason="Completed"`
6. Then run `bv --robot-next` again and repeat — keep going until you run out of actionable tasks or context

## Key context

- **Monorepo structure** (from SPEC.md §Part 2 directory tree):
  - Rust: `Cargo.toml` workspace root → `crates/{common,daemon,cli,relay}`
  - TypeScript: `pnpm-workspace.yaml` + `turbo.json` → `packages/{editor,web,shared,mcp-server}`
  - Tooling: Biome (TS lint/fmt), clippy+rustfmt (Rust), vitest (TS test), cargo nextest (Rust test)
- **Edition**: Rust 2021, resolver=2
- **TS**: pnpm strict mode, Turborepo for orchestration

## Critical rules for parallel work

**Multiple agents are working concurrently on the same branch.**

- **Work autonomously, there is no human for you to interact with.**
- **Do not mark something as complete if it's not actually done.**
- **Only touch files directly related to your task.** Don't reorganize, rename, or "improve" files outside your scope.
- **If a file you need to edit already exists** (created by another agent), read it first and add to it — don't overwrite.
- **If your task is blocked** (dependency not yet created by another agent), skip it and move to the next `bv --robot-next` suggestion.
- **Don't run the full test suite** — only test your own work.

## Before ending your session

See `AGENTS.md` The short version:

```bash
git add <your-files>        # Only YOUR files — never git add . or git add -A
git commit -m "feat: ..."   # Conventional commit
br sync --flush-only        # Export beads changes
```
