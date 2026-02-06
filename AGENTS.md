# AGENTS.md

- Local-first collaborative markdown with git sync and first-class agent support.
- Spec-only phase — no code yet.

## Commands

```
# None yet — project is pre-implementation
```

## How It Works

- Yjs CRDT in a local daemon, CodeMirror 6 web editor connects via y-websocket
- Relay server for multi-user collab; local replica is authoritative
- File watcher syncs local `.md` files ↔ CRDT state
- Git sync via leader election among daemons
- Agents are first-class collaborators with attribution (CRDT origin tags + server-side log)

## Structure

- `SPEC.md` — complete specification (6 parts: product, architecture, data, API, dev tooling, ops)
- `REVIEW.md` — spec review tracker with resolution status

## Git

- Check `git status`/`git diff` before commits
- Atomic commits; push only when asked
- Never destructive ops (`reset --hard`, `force push`, `checkout .`, `stash`, `restore`) without explicit consent — other agents may be editing the same worktree
- Conventional Commits: `feat:`, `fix:`, `docs:`, `refactor:`

## Critical Thinking

- Read more code when stuck
- Document unexpected behavior
- Call out conflicts between instructions

## Parallel Agent Work

Multiple agents work concurrently in the same worktree and branch.

- **Only fix what you broke.** If tests fail that are unrelated to your changes, leave them — another agent likely has that in progress.
- Run tests scoped to your work (e.g. the module you changed), not the full suite, unless asked.
- If pre-existing failures block your work, create a `br` issue — don't try to fix them.
- Claim work with `br update <id> --status=in_progress` so other agents can see it's taken.
- Expect files to change under you. Re-read before editing if your context is stale.
- Keep commits small and focused — avoid touching files outside your task to minimize merge pain.

## Engineering

- Small files (<500 LOC), descriptive paths, current header comments
- Fix root causes, not symptoms
- Simplicity > cleverness (even if it means bigger refactors)
- Aim for 100% test coverage

## UI Testing

- Use the `dev-browser` skill for testing web UI changes. Headless browser
automation with Playwright. Start server, take screenshots, verify DOM state.

<!-- br-agent-instructions-v1 -->

---

## Beads Workflow Integration

This project uses [beads_rust](https://github.com/Dicklesworthstone/beads_rust) (`br`/`bd`) for issue tracking and [beads_viewer](https://github.com/Dicklesworthstone/beads_viewer) (`bv`) for graph-aware triage. Issues are stored in `.beads/` and tracked in git.

### Triage (bv — read-only intelligence)

**Use `bv --robot-*` flags to decide what to work on. Never run bare `bv` — it launches an interactive TUI that blocks agent sessions.**

```bash
# Pick next task (graph-aware: considers dependencies, PageRank, critical path)
bv --robot-next                        # Single top pick + claim command

# Full triage (ranked picks, quick wins, blockers-to-clear, project health)
bv --robot-triage --format toon        # --format toon = token-optimized output

# Planning & analysis
bv --robot-plan                        # Parallel execution tracks with unblock lists
bv --robot-insights                    # PageRank, betweenness, cycles, critical path
bv --robot-alerts                      # Stale issues, blocking cascades, priority mismatches
```

### Mutations (br — create, update, close)

```bash
# List and search
br list --status=open # All open issues
br show <id>          # Full issue details with dependencies
br search "keyword"   # Full-text search

# Create and update
br create --title="..." --description="..." --type=task --priority=2
br update <id> --status=in_progress
br close <id> --reason="Completed"
br close <id1> <id2>  # Close multiple issues at once

# Sync with git
br sync --flush-only  # Export DB to JSONL
br sync --status      # Check sync status
```

### Workflow Pattern

1. **Start**: Run `bv --robot-next` to get the highest-impact actionable task
2. **Claim**: Use `br update <id> --status=in_progress`
3. **Work**: Implement the task
4. **Complete**: Use `br close <id>`
5. **Sync**: Always run `br sync --flush-only` at session end

### Key Concepts

- **Dependencies**: Issues can block other issues. `bv --robot-next` factors in the full dependency graph to pick optimal work.
- **Priority**: P0=critical, P1=high, P2=medium, P3=low, P4=backlog (use numbers 0-4, not words)
- **Types**: task, bug, feature, epic, chore, docs, question
- **Blocking**: `br dep add <issue> <depends-on>` to add dependencies

### Session Protocol

**Before ending any session, run this checklist:**

```bash
git status              # Check what changed
git add <files>         # Stage code changes
br sync --flush-only    # Export beads changes to JSONL
git commit -m "..."     # Commit everything
git push                # Push to remote
```

### Best Practices

- Use `bv --robot-next` at session start — it picks the highest-impact unblocked task
- For broader context, run `bv --robot-triage --format toon` to see ranked recommendations, quick wins, and blockers worth clearing
- Update status as you work (in_progress → closed)
- Create new issues with `br create` when you discover tasks
- Use descriptive titles and set appropriate priority/type
- Always sync before ending session

<!-- end-br-agent-instructions -->

## Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd sync
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
