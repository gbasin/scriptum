#!/usr/bin/env bash
# Scriptum Stop hook — check for unsynced changes at session end.
# Warns if there are pending changes that haven't been synced to CRDT.
set -euo pipefail

echo "=== Scriptum Session End ==="
echo ""

# Check for pending/unsynced changes.
STATUS=$(scriptum status 2>/dev/null || echo "")
if [ -n "$STATUS" ]; then
    echo "$STATUS"
    if echo "$STATUS" | grep -qi "pending\|unsynced\|dirty"; then
        echo ""
        echo "⚠ Warning: You may have unsynced changes."
        echo "  Ensure the daemon is running and changes are persisted."
    fi
else
    echo "(Scriptum status unavailable — daemon not running)"
fi

echo ""
echo "Overlap check:"
scriptum conflicts 2>/dev/null || echo "  (no conflicts or daemon not running)"
