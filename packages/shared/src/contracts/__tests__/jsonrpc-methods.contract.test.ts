import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type { RpcParamsMap } from "../../protocol/rpc";

const __dirname = dirname(fileURLToPath(import.meta.url));
const contract = JSON.parse(
  readFileSync(
    resolve(__dirname, "../../../../../contracts/jsonrpc-methods.json"),
    "utf-8",
  ),
);

// Compile-time proof that these keys exist in RpcParamsMap.
const rpcMethodKeys: Record<keyof RpcParamsMap, true> = {
  "workspace.list": true,
  "workspace.open": true,
  "workspace.create": true,
  "doc.read": true,
  "doc.edit": true,
  "doc.edit_section": true,
  "doc.sections": true,
  "doc.tree": true,
  "doc.search": true,
  "doc.diff": true,
  "doc.history": true,
  "agent.whoami": true,
  "agent.status": true,
  "agent.conflicts": true,
  "agent.list": true,
  "agent.claim": true,
  "doc.bundle": true,
  "git.status": true,
  "git.sync": true,
  "git.configure": true,
};

describe("jsonrpc-methods contract", () => {
  it("RpcParamsMap keys match implemented methods (minus daemon-only)", () => {
    const daemonOnly = ["rpc.ping", "daemon.shutdown"];
    const expected = (contract.implemented_methods as string[])
      .filter((m) => !daemonOnly.includes(m))
      .sort();
    expect(Object.keys(rpcMethodKeys).sort()).toEqual(expected);
  });

  it("MCP-to-daemon mapping values are implemented methods", () => {
    const mapping = contract.mcp_to_daemon as Record<string, string>;
    for (const [mcpTool, daemonMethod] of Object.entries(mapping)) {
      expect(
        contract.implemented_methods,
        `${mcpTool} maps to ${daemonMethod} which is not implemented`,
      ).toContain(daemonMethod);
    }
  });
});
