# Contract Manifests

JSON files in this directory are the **single source of truth** for cross-boundary
constants shared between Rust and TypeScript. Both sides have tests that read
these files at test time and assert their code matches.

## Pattern

1. Define the canonical value in a `contracts/*.json` file.
2. In Rust, create constants (or enums) that mirror the JSON values. Write a
   `#[test]` that deserializes the JSON and asserts equality.
3. In TypeScript, create `const` objects/arrays that mirror the JSON values.
   Write a Vitest test that reads the JSON and asserts equality.
4. Any drift in either language causes its own test suite to fail — no codegen
   step required.

## Files

| File | What it locks down | Rust test | TS test |
|------|--------------------|-----------|---------|
| `jsonrpc-methods.json` | 21 implemented + 7 planned RPC methods, MCP mapping | `crates/common/tests/rpc_methods_contract.rs` | `packages/shared/src/contracts/__tests__/jsonrpc-methods.contract.test.ts` |
| `error-codes.json` | 17 error codes with HTTP status + retryability | `crates/relay/src/error.rs` (unit tests) | `packages/shared/src/contracts/__tests__/error-codes.contract.test.ts` |
| `roles.json` | 3 workspace roles, 2 default-assignable roles | `crates/relay/src/auth/middleware.rs` (unit tests) | `packages/shared/src/contracts/__tests__/roles.contract.test.ts` |
| `daemon-ports.json` | Port 39091, host, WS/whoami paths | `crates/daemon/src/runtime.rs` (unit test) | `packages/shared/src/contracts/__tests__/daemon-ports.contract.test.ts` |
| `storage-keys.json` | 8 localStorage keys (session + OAuth flow) | N/A (browser-only) | `packages/shared/src/contracts/__tests__/storage-keys.contract.test.ts` |
| `ws-protocol.json` | 8 message types, protocol versions | `crates/common/tests/ws_protocol_contract.rs` | `packages/shared/src/contracts/__tests__/ws-protocol.contract.test.ts` |
| `desktop-menu.json` | 9 menu IDs, 5 shortcuts, action event names | `packages/desktop/src-tauri/src/menu.rs` (unit tests) | `packages/shared/src/contracts/__tests__/desktop-menu.contract.test.ts` |

## Existing pattern

`packages/desktop/tauri-auth-contract.json` was the first contract file and
pioneered this approach with bidirectional Rust + TS tests for Tauri IPC commands.

## Verification

```bash
# Rust tests — validates all Rust code against contract JSONs
cargo test --workspace

# TS tests — validates all TS code against contract JSONs
cd packages/shared && npx vitest run
cd packages/mcp-server && npx vitest run
```
