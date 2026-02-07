#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-smoke}"

run_test() {
  local label="$1"
  shift
  echo "==> ${label}"
  "$@"
}

run_stress_profile() {
  local label="$1"
  shift
  run_test "${label}" env "$@" cargo test -p scriptum-relay websocket_load_stress_weekly_and_pre_release_profile -- --ignored --nocapture
}

run_reconnect_profile() {
  local label="$1"
  shift
  run_test "${label}" env "$@" cargo test -p scriptum-relay websocket_load_reconnect_storm_weekly_and_pre_release_profile -- --ignored --nocapture
}

case "${MODE}" in
  smoke)
    run_test "Smoke: websocket stress profile" cargo test -p scriptum-relay websocket_load_stress_smoke_profile -- --nocapture
    run_test "Smoke: reconnect storm profile" cargo test -p scriptum-relay websocket_load_reconnect_storm_smoke_profile -- --nocapture
    ;;
  weekly|pre-release)
    run_stress_profile \
      "Scenario 1: 1000 concurrent sessions (capacity + ack p95)" \
      SCRIPTUM_RELAY_LOAD_CONCURRENT_SESSIONS=1000 \
      SCRIPTUM_RELAY_LOAD_UPDATES_PER_SECOND=10 \
      SCRIPTUM_RELAY_LOAD_SOAK_SECONDS=120 \
      SCRIPTUM_RELAY_LOAD_ACK_P95_TARGET_MS=500 \
      SCRIPTUM_RELAY_LOAD_MIN_UPDATES_RATIO=0.80 \
      SCRIPTUM_RELAY_LOAD_MAX_ERROR_RATE=0.01 \
      SCRIPTUM_RELAY_LOAD_ENFORCE_MEMORY_LIMIT=false

    run_stress_profile \
      "Scenario 2: 50 updates/sec on one document for 10 minutes" \
      SCRIPTUM_RELAY_LOAD_CONCURRENT_SESSIONS=64 \
      SCRIPTUM_RELAY_LOAD_UPDATES_PER_SECOND=50 \
      SCRIPTUM_RELAY_LOAD_SOAK_SECONDS=600 \
      SCRIPTUM_RELAY_LOAD_ACK_P95_TARGET_MS=500 \
      SCRIPTUM_RELAY_LOAD_MIN_UPDATES_RATIO=0.90 \
      SCRIPTUM_RELAY_LOAD_MAX_ERROR_RATE=0.01 \
      SCRIPTUM_RELAY_LOAD_ENFORCE_MEMORY_LIMIT=false

    run_reconnect_profile \
      "Scenario 3: reconnect storm (500 clients, 10k pending updates)" \
      SCRIPTUM_RELAY_RECONNECT_STORM_CLIENTS=500 \
      SCRIPTUM_RELAY_RECONNECT_STORM_PENDING_UPDATES=10000 \
      SCRIPTUM_RELAY_RECONNECT_STORM_ACK_P95_TARGET_MS=500 \
      SCRIPTUM_RELAY_RECONNECT_STORM_CATCHUP_P95_TARGET_MS=2000 \
      SCRIPTUM_RELAY_RECONNECT_STORM_MAX_ERROR_RATE=0.01

    run_stress_profile \
      "Scenario 4: 1-hour soak (200 sessions, 10 updates/sec/doc)" \
      SCRIPTUM_RELAY_LOAD_CONCURRENT_SESSIONS=200 \
      SCRIPTUM_RELAY_LOAD_UPDATES_PER_SECOND=10 \
      SCRIPTUM_RELAY_LOAD_SOAK_SECONDS=3600 \
      SCRIPTUM_RELAY_LOAD_ACK_P95_TARGET_MS=500 \
      SCRIPTUM_RELAY_LOAD_MIN_UPDATES_RATIO=0.90 \
      SCRIPTUM_RELAY_LOAD_MAX_ERROR_RATE=0.01 \
      SCRIPTUM_RELAY_LOAD_ENFORCE_MEMORY_LIMIT=true
    ;;
  *)
    cat <<'EOF'
Usage: tests/load/run-relay-load.sh [smoke|weekly|pre-release]

smoke:
  Fast local validation for stress + reconnect profiles.

weekly | pre-release:
  Full SLA-oriented relay load scenarios.
EOF
    exit 1
    ;;
esac
