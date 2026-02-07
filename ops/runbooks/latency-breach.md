# Runbook: Latency Breach

## Alert
- Alert: `ScriptumRelayLatencyBreach`
- Condition: `quantile_over_time(0.95, sync_ack_latency_ms[3h]) > 500`
- Severity: `ticket`

## Checks
1. Confirm sustained latency:
   - `quantile_over_time(0.95, sync_ack_latency_ms[3h])`
2. Correlate with request durations:
   - `sum(rate(relay_ws_duration_ms_sum{endpoint="yjs_update"}[15m])) / clamp_min(sum(rate(relay_ws_duration_ms_count{endpoint="yjs_update"}[15m])), 1)`
3. Check DB/query latency and relay resource saturation.

## Remediation plan
1. Profile hot paths in websocket update handling.
2. Inspect DB query plans and add/adjust indexes as needed.
3. Evaluate batching and queue handling in sequencer/outbox paths.
4. Open optimization work item with before/after benchmarks.

## Exit criteria
1. p95 sync ack latency is `<= 500ms` over a sustained 3-hour window.
2. Regression cause is identified and linked to an implementation issue.
