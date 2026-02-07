# Relay Load Test Suite

This directory contains script-based load scenarios for relay WebSocket SLAs from `SPEC.md`.

## Scenarios

1. `1000` concurrent sessions connected to relay.
2. `50` updates/sec sustained on one document for `10` minutes.
3. Reconnect storm with `500` clients and `10k` pending updates catch-up.
4. `1` hour soak with `200` sessions at `10` updates/sec/doc.

## Commands

```bash
# Fast local validation
tests/load/run-relay-load.sh smoke

# Full load suite for weekly or pre-release validation
tests/load/run-relay-load.sh weekly
tests/load/run-relay-load.sh pre-release
```

## Metrics Output

Load tests emit `LOAD_REPORT` lines with:

- Throughput (`updates/sec`)
- Ack latency percentiles (`p50`, `p95`, `p99`)
- Ack error rate
- Reconnect catch-up latency percentiles (`p50`, `p95`, `p99`)
- Memory baseline and peak RSS (when available on Linux)
