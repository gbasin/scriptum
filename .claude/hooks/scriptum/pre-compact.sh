#!/usr/bin/env bash
# Scriptum PreCompact hook — preserve context across /compact.
set -euo pipefail

echo "=== Scriptum Context (preserved across compaction) ==="
scriptum whoami 2>/dev/null || echo "(agent identity unavailable)"
echo ""
scriptum status 2>/dev/null || echo "(status unavailable)"
echo ""
scriptum conflicts 2>/dev/null || true
