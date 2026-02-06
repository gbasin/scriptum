#!/usr/bin/env bash
# Scriptum Stop hook — check for unsynced changes.
set -euo pipefail

echo "=== Scriptum Session End ==="
scriptum status 2>/dev/null || echo "(status unavailable)"
scriptum conflicts 2>/dev/null || true
