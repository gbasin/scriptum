#!/usr/bin/env bash
# Scriptum PreCompact hook — preserve context across /compact.
set -euo pipefail

echo "=== Scriptum Context Snapshot (preserve after /compact) ==="
echo "Keep this block so Scriptum state survives compaction."
echo ""
echo "=== Scriptum Agent State ==="
scriptum whoami 2>/dev/null || echo "(agent identity unavailable)"
echo ""
echo "=== Scriptum Workspace Status ==="
scriptum status 2>/dev/null || echo "(status unavailable)"
echo ""
echo "=== Scriptum Active Overlaps ==="
scriptum conflicts 2>/dev/null || true
echo ""
echo "=== Scriptum CLI Quick Reference ==="
echo "  scriptum read <doc>           Read document or section"
echo "  scriptum edit <doc>           Edit document or section"
echo "  scriptum status               Show agent state and overlaps"
echo "  scriptum conflicts            Show section overlap warnings"
echo "  scriptum claim <section>      Claim advisory lease"
