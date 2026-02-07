#!/usr/bin/env bash
# Scriptum PreToolUse hook — warn about .md file edits.
# Triggered on Write|Edit tools. Warns about section overlaps
# and suggests using scriptum edit for attribution.
set -euo pipefail

# Parse the tool input to check if target is a .md file.
# Claude Code passes tool input as JSON via stdin.
INPUT=$(cat)
FILE=$(echo "$INPUT" | grep -oP '"file_path"\s*:\s*"[^"]*\.md"' || true)

if [ -n "$FILE" ]; then
    echo "⚠ Scriptum: Direct .md edit detected."
    echo "  Prefer \`scriptum edit\` for CRDT attribution and section-level sync."
    echo ""
    echo "Section overlap check:"
    scriptum conflicts 2>/dev/null || echo "  (overlap check unavailable — daemon not running)"
fi
