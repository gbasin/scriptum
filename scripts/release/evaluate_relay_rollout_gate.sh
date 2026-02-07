#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  evaluate_relay_rollout_gate.sh \
    --baseline-crash-rate <float> \
    --current-crash-rate <float> \
    --current-error-rate <float> \
    [--crash-multiplier-threshold <float>] \
    [--error-rate-threshold <float>]

Fails rollout when:
  - current error rate is above threshold (default: 0.01 = 1%)
  - current crash rate is above baseline * threshold (default: 2x)
EOF
}

baseline_crash_rate=""
current_crash_rate=""
current_error_rate=""
crash_multiplier_threshold="2.0"
error_rate_threshold="0.01"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --baseline-crash-rate)
      baseline_crash_rate="${2:-}"
      shift 2
      ;;
    --current-crash-rate)
      current_crash_rate="${2:-}"
      shift 2
      ;;
    --current-error-rate)
      current_error_rate="${2:-}"
      shift 2
      ;;
    --crash-multiplier-threshold)
      crash_multiplier_threshold="${2:-}"
      shift 2
      ;;
    --error-rate-threshold)
      error_rate_threshold="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$baseline_crash_rate" || -z "$current_crash_rate" || -z "$current_error_rate" ]]; then
  echo "Missing required arguments." >&2
  usage >&2
  exit 2
fi

awk -v baseline="$baseline_crash_rate" \
    -v current_crash="$current_crash_rate" \
    -v current_error="$current_error_rate" \
    -v crash_threshold="$crash_multiplier_threshold" \
    -v error_threshold="$error_rate_threshold" '
  BEGIN {
    if (current_error > error_threshold) {
      printf("ROLLBACK: error rate %.6f exceeds threshold %.6f\n", current_error, error_threshold) > "/dev/stderr";
      exit 11;
    }

    if (baseline <= 0) {
      if (current_crash > 0) {
        printf("ROLLBACK: baseline crash rate is %.6f and current crash rate is %.6f\n", baseline, current_crash) > "/dev/stderr";
        exit 12;
      }
      printf("PASS: baseline and current crash rates are both zero\n");
      exit 0;
    }

    allowed_crash = baseline * crash_threshold;
    if (current_crash > allowed_crash) {
      printf("ROLLBACK: crash rate %.6f exceeds %.2fx baseline %.6f (allowed %.6f)\n", current_crash, crash_threshold, baseline, allowed_crash) > "/dev/stderr";
      exit 13;
    }

    printf("PASS: crash/error thresholds satisfied (crash=%.6f baseline=%.6f error=%.6f)\n", current_crash, baseline, current_error);
    exit 0;
  }
'
