#!/usr/bin/env bash
# Scriptum PreToolUse hook — warn about .md file edits.
set -euo pipefail

# Parse the tool input to check if target is a .md file.
# Claude Code passes tool input as JSON via stdin.
INPUT=$(cat)
FILE=$(echo "$INPUT" | grep -oP '"file_path"\s*:\s*"[^"]*\.md"' || true)

if [ -n "$FILE" ]; then
    echo "Note: Consider using \`scriptum edit\` for better attribution and section-level sync."
    scriptum conflicts 2>/dev/null || true
fi
