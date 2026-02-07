#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  evaluate_desktop_ring_gate.sh \
    --baseline-crash-rate <float> \
    --current-crash-rate <float> \
    [--crash-multiplier-threshold <float>]

Fails promotion when current crash rate exceeds baseline * threshold.
Default threshold is 2x.
EOF
}

baseline_crash_rate=""
current_crash_rate=""
crash_multiplier_threshold="2.0"

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
    --crash-multiplier-threshold)
      crash_multiplier_threshold="${2:-}"
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

if [[ -z "$baseline_crash_rate" || -z "$current_crash_rate" ]]; then
  echo "Missing required arguments." >&2
  usage >&2
  exit 2
fi

awk -v baseline="$baseline_crash_rate" \
    -v current_crash="$current_crash_rate" \
    -v crash_threshold="$crash_multiplier_threshold" '
  BEGIN {
    if (baseline <= 0) {
      if (current_crash > 0) {
        printf("KILL_SWITCH: baseline crash rate is %.6f and current crash rate is %.6f\n", baseline, current_crash) > "/dev/stderr";
        exit 21;
      }
      printf("PASS: baseline and current crash rates are both zero\n");
      exit 0;
    }

    allowed_crash = baseline * crash_threshold;
    if (current_crash > allowed_crash) {
      printf("KILL_SWITCH: crash rate %.6f exceeds %.2fx baseline %.6f (allowed %.6f)\n", current_crash, crash_threshold, baseline, allowed_crash) > "/dev/stderr";
      exit 22;
    }

    printf("PASS: ring promotion threshold satisfied (crash=%.6f baseline=%.6f)\n", current_crash, baseline);
    exit 0;
  }
'
