# Scriptum Ops Artifacts

This directory contains production operations artifacts derived from the SPEC observability section.

## Alert rules
- `ops/alerting/relay-alerts.yml`

## Runbooks
- `ops/runbooks/availability-drop.md`
- `ops/runbooks/sync-error-spike.md`
- `ops/runbooks/outbox-growth.md`
- `ops/runbooks/latency-breach.md`

Paging alerts (`severity: page`) include explicit `runbook` annotations in the alert rules file.
