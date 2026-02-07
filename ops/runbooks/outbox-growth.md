# Runbook: Outbox Growth Spike

## Alert
- Alert: `ScriptumRelayOutboxGrowthSpike`
- Condition: `sum(outbox_depth)` grows `10x` over `15m` and is at least `100`
- Severity: `page`

## Immediate checks (first 5 minutes)
1. Confirm growth trend:
   - `sum(outbox_depth)`
   - `sum(outbox_depth offset 15m)`
2. Identify top workspaces:
   - `topk(10, outbox_depth)`
3. Verify relay connectivity and websocket stability.

## Diagnosis
1. Check relay logs for sync ACK failures and timeout spikes.
2. Check DB write latency and lock contention.
3. Check if downstream git sync or daemon recovery is lagging.
4. Determine if growth is global or isolated to specific workspaces.

## Mitigation
1. Resolve relay connectivity failures first (network, LB, pod health).
2. Reduce backpressure by scaling relay and/or database.
3. Temporarily rate-limit noisy workspaces if one tenant dominates queue depth.
4. If needed, fail over to healthy region/cluster.

## Exit criteria
1. Outbox growth ratio drops below `10x` and absolute depth trends downward.
2. Top workspaces show sustained queue drain.
3. Backpressure source is documented with follow-up remediation task.
