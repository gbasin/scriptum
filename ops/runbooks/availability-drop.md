# Runbook: Relay Availability Drop

## Alert
- Alert: `ScriptumRelayAvailabilityDrop`
- Condition: availability `< 99.5%` over `15m`
- Severity: `page`

## Immediate checks (first 5 minutes)
1. Confirm alert scope in Prometheus:
   - `sum(rate(relay_request_rate_total[15m]))`
   - `sum(rate(relay_ws_rate_total[15m]))`
   - `sum(rate(relay_request_errors_total[15m]))`
   - `sum(rate(relay_ws_errors_total[15m]))`
2. Check relay health endpoint: `/healthz`.
3. Check recent deploys/config changes and restart activity.

## Diagnosis
1. Break down failing endpoints:
   - `sum(rate(relay_request_errors_total[15m])) by (method, endpoint)`
   - `sum(rate(relay_ws_errors_total[15m])) by (endpoint)`
2. Inspect relay logs for elevated `error_code` and latency.
3. Check PostgreSQL health (connections, saturation, slow queries).
4. Check pod/container status (OOM, crash loops, CPU throttling).

## Mitigation
1. Scale relay pods if saturation is present.
2. Roll back the latest release if errors correlate with deploy.
3. Reduce load (rate limit non-critical endpoints) if needed.
4. Restore DB capacity or fail over if DB is degraded.

## Exit criteria
1. Availability is `>= 99.5%` for at least 15 minutes.
2. Error rates return to normal baseline.
3. Incident timeline and root cause are documented.
