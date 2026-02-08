#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

usage() {
  cat <<'USAGE'
Usage: scripts/ci/pipeline.sh <target>

Targets:
  help
  lint
  test-ts
  test-rust
  golden
  security
  integration-daemon-watcher
  integration-relay-websocket
  integration-git-worker
  property-crdt
  compatibility-rest
  compatibility-websocket
  compatibility-json-rpc
  load-relay-websocket
  fast
  full
USAGE
}

log() {
  printf "==> %s\n" "$*"
}

run() {
  echo "+ $*"
  "$@"
}

has_cargo_subcommand() {
  cargo "$1" --version >/dev/null 2>&1
}

run_rust_test_suite() {
  if has_cargo_subcommand nextest; then
    run cargo nextest run --workspace --all-targets
    return
  fi

  log "cargo-nextest is not installed; falling back to cargo test."
  run cargo test --workspace --all-targets
}

run_cargo_deny() {
  if has_cargo_subcommand deny; then
    run cargo deny check
    return
  fi

  echo "cargo-deny is required for target 'security'." >&2
  echo "Install with: cargo install --locked cargo-deny" >&2
  exit 127
}

target="${1:-}"
if [[ -z "$target" ]]; then
  usage
  exit 2
fi

case "$target" in
  help)
    usage
    ;;

  lint)
    run pnpm check
    run cargo fmt --all -- --check
    run cargo clippy --workspace --all-targets
    run pnpm -r --if-present run typecheck
    ;;

  test-ts)
    run pnpm test
    ;;

  test-rust)
    run_rust_test_suite
    ;;

  golden)
    run cargo test -p scriptum-common --test golden_runner -- --nocapture
    ;;

  security)
    run cargo test -p scriptum-relay --test security_authz -- --nocapture
    run cargo test -p scriptum-daemon --test security_path_traversal -- --nocapture
    run cargo test -p scriptum-relay --test security_token -- --nocapture
    run pnpm --filter @scriptum/editor exec vitest run src/live-preview/security.test.ts
    run cargo test -p scriptum-relay start_rate_limits_and_sets_retry_after_header -- --nocapture
    run_cargo_deny
    ;;

  integration-daemon-watcher)
    run cargo test -p scriptum-daemon --test daemon_file_watcher_integration -- --nocapture
    ;;

  integration-relay-websocket)
    run cargo test -p scriptum-relay websocket_integration_ack_and_broadcast_to_other_subscriber -- --nocapture
    ;;

  integration-git-worker)
    run cargo test -p scriptum-daemon git::worker::tests -- --nocapture
    run cargo test -p scriptum-daemon git::leader::tests -- --nocapture
    run cargo test -p scriptum-daemon --test git_worker_e2e_integration -- --nocapture
    ;;

  property-crdt)
    run cargo test -p scriptum-daemon --test crdt_convergence_property -- --nocapture
    run cargo test -p scriptum-daemon --test diff_to_yjs_property -- --nocapture
    ;;

  compatibility-rest)
    run cargo test -p scriptum-relay workspace_rest_response_shape_matches_contract -- --nocapture
    ;;

  compatibility-websocket)
    run cargo test -p scriptum-relay compatibility_matrix_accepts_all_supported_versions_and_keeps_unique_order -- --nocapture
    run cargo test -p scriptum-relay create_sync_session_returns_expected_contract -- --nocapture
    run cargo test -p scriptum-relay create_sync_session_rejects_unsupported_protocol_with_upgrade_required -- --nocapture
    ;;

  compatibility-json-rpc)
    run cargo test -p scriptum-daemon --test jsonrpc_contract_integration jsonrpc_version_matrix_supports_current_and_rejects_legacy -- --nocapture
    ;;

  load-relay-websocket)
    run tests/load/run-relay-load.sh pre-release
    ;;

  fast)
    run "$0" lint
    run "$0" test-ts
    ;;

  full)
    run "$0" lint
    run "$0" test-ts
    run "$0" test-rust
    run "$0" golden
    run "$0" security
    run "$0" integration-daemon-watcher
    run "$0" integration-relay-websocket
    run "$0" integration-git-worker
    run "$0" property-crdt
    run "$0" compatibility-rest
    run "$0" compatibility-websocket
    run "$0" compatibility-json-rpc
    ;;

  *)
    echo "Unknown target: $target" >&2
    usage
    exit 2
    ;;
esac
