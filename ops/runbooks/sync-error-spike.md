# Runbook: Sync Error Spike

## Alert
- Alert: `ScriptumRelaySyncErrorSpike`
- Condition: sync error ratio `> 1%` over `10m` on `yjs_update`
- Severity: `page`

## Immediate checks (first 5 minutes)
1. Confirm ratio and traffic volume:
   - `sum(rate(relay_ws_errors_total{endpoint="yjs_update"}[10m]))`
   - `sum(rate(relay_ws_rate_total{endpoint="yjs_update"}[10m]))`
2. Validate relay and DB are reachable from running pods.
3. Check sequence-gap growth:
   - `increase(sequence_gap_count[10m])`

## Diagnosis
1. Inspect relay logs for websocket error clusters by workspace.
2. Check sequencer/database latency and failed writes.
3. Verify connection churn and reconnect storms.
4. Confirm no malformed client frames from a new client build.

## Mitigation
1. If DB pressure is high, scale DB resources or throttle heavy tenants.
2. If relay CPU/memory is saturated, scale relay pods.
3. Roll back recent relay/client release if regression started after deploy.
4. If one workspace is pathological, isolate/throttle to protect global health.

## Exit criteria
1. Error ratio drops to `<= 1%` over 10 minutes.
2. `sequence_gap_count` growth returns to baseline.
3. Follow-up issue created for root-cause fix if temporary mitigations were used.
