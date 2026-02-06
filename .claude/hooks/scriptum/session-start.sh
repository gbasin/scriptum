#!/usr/bin/env bash
# Scriptum SessionStart hook — inject workspace context into Claude session.
set -euo pipefail

echo "=== Scriptum Agent State ==="
scriptum whoami 2>/dev/null || echo "(agent identity unavailable)"
echo ""
echo "=== Scriptum Workspace Status ==="
scriptum status 2>/dev/null || echo "(scriptum daemon not running)"
echo ""
echo "=== Scriptum Overlap Warnings ==="
scriptum conflicts 2>/dev/null || true
echo ""
echo "=== Scriptum CLI Quick Reference ==="
echo "  scriptum read <doc>           Read document or section"
echo "  scriptum edit <doc>           Edit document or section"
echo "  scriptum tree <doc>           Show section tree"
echo "  scriptum ls                   List workspace documents"
echo "  scriptum status               Show agent state and overlaps"
echo "  scriptum conflicts            Show section overlap warnings"
echo "  scriptum claim <section>      Claim advisory lease"
echo "  scriptum blame <doc>          CRDT-based attribution"
echo "  scriptum bundle <doc>         Context bundling for agents"
echo "  scriptum agents               List active agents"
