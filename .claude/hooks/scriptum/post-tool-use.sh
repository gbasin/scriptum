#!/usr/bin/env bash
# Scriptum PostToolUse hook — confirm file sync after .md edits.
set -euo pipefail

# Parse tool input for .md file path.
INPUT=$(cat)
FILE=$(echo "$INPUT" | grep -oP '"file_path"\s*:\s*"[^"]*\.md"' || true)

if [ -n "$FILE" ]; then
    echo "Scriptum: File watcher will sync this change to CRDT."
    scriptum conflicts 2>/dev/null || true
fi
