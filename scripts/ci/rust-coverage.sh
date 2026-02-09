#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

DEFAULT_CARGO="$(command -v cargo || true)"
RUSTUP_CARGO="${HOME}/.cargo/bin/cargo"
CARGO_BIN="${SCRIPTUM_CARGO_BIN:-$DEFAULT_CARGO}"

if [[ -z "$CARGO_BIN" ]]; then
  echo "cargo is required to run Rust coverage." >&2
  exit 127
fi

if ! "$CARGO_BIN" llvm-cov --version >/dev/null 2>&1; then
  echo "cargo-llvm-cov is required to run Rust coverage." >&2
  echo "Install with: cargo install cargo-llvm-cov" >&2
  exit 127
fi

mkdir -p coverage/rust

run_coverage() {
  local bin="$1"
  "$bin" llvm-cov --workspace --all-targets --lcov --output-path coverage/rust/lcov.info
}

if ! run_coverage "$CARGO_BIN"; then
  if [[ "$CARGO_BIN" != "$RUSTUP_CARGO" && -x "$RUSTUP_CARGO" ]]; then
    echo "Retrying Rust coverage with rustup cargo shim: $RUSTUP_CARGO" >&2
    run_coverage "$RUSTUP_CARGO"
    exit 0
  fi
  exit 1
fi
