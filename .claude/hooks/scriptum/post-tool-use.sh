#!/usr/bin/env bash
# Scriptum PostToolUse hook — confirm file sync after .md edits.
# Triggered on Write|Edit tools. Confirms file watcher will sync
# the change and reports any active section conflicts.
set -euo pipefail

# Parse tool input for .md file path.
INPUT=$(cat)
FILE=$(echo "$INPUT" | grep -oP '"file_path"\s*:\s*"[^"]*\.md"' || true)

if [ -n "$FILE" ]; then
    echo "Scriptum: File watcher will sync this change to CRDT."
    echo "  Run \`scriptum status\` to verify sync completed."
    CONFLICTS=$(scriptum conflicts 2>/dev/null || true)
    if [ -n "$CONFLICTS" ]; then
        echo ""
        echo "Active conflicts:"
        echo "$CONFLICTS"
    fi
fi
