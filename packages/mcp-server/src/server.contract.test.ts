import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { describe, expect, it } from "vitest";

import type { DaemonClient } from "./daemon-client";
import { createServer } from "./server";

const MCP_TO_DAEMON_CONTRACT: Record<string, string> = {
  scriptum_status: "agent.status",
  scriptum_subscribe: "agent.status",
  scriptum_read: "doc.read",
  scriptum_edit: "doc.edit",
  scriptum_list: "doc.tree",
  scriptum_tree: "doc.sections",
  scriptum_conflicts: "agent.conflicts",
  scriptum_history: "doc.diff",
  scriptum_agents: "agent.list",
  scriptum_claim: "agent.claim",
  scriptum_bundle: "doc.bundle",
};

describe("mcp tool contract", () => {
  it("exposes the exact tool set expected by the daemon RPC contract", async () => {
    const [clientTransport, serverTransport] =
      InMemoryTransport.createLinkedPair();
    const daemonClient: DaemonClient = {
      request: async () => ({}),
    };
    const server = createServer({
      daemonClient,
      transportFactory: () => serverTransport,
    });
    const client = new Client({
      name: "contract-checker",
      version: "1.0.0",
    });

    await server.start();

    try {
      await client.connect(clientTransport);
      const tools = await client.listTools();
      const names = tools.tools.map((tool) => tool.name).sort();

      expect(names).toEqual([
        "scriptum_agents",
        "scriptum_bundle",
        "scriptum_claim",
        "scriptum_conflicts",
        "scriptum_edit",
        "scriptum_history",
        "scriptum_list",
        "scriptum_read",
        "scriptum_status",
        "scriptum_subscribe",
        "scriptum_tree",
      ]);
    } finally {
      await client.close();
      await server.close();
    }
  });

  it("maps MCP passthrough tools to the expected daemon methods", async () => {
    const [clientTransport, serverTransport] =
      InMemoryTransport.createLinkedPair();
    const calls: Array<{ method: string; params: unknown }> = [];
    const daemonClient: DaemonClient = {
      request: async (method, params) => {
        calls.push({ method, params: params ?? null });
        if (method === "agent.status") {
          return {
            active_sessions: [],
            change_token: "tok-1",
            echoed_params: params ?? null,
          };
        }
        return {
          rpc_method: method,
          echoed_params: params ?? null,
        };
      },
    };
    const server = createServer({
      daemonClient,
      transportFactory: () => serverTransport,
    });
    const client = new Client({
      name: "contract-checker",
      version: "1.0.0",
    });

    await server.start();

    try {
      await client.connect(clientTransport);

      const statusResult = await client.callTool({
        name: "scriptum_status",
        arguments: { workspace_id: "ws" },
      });
      const subscribeResult = await client.callTool({
        name: "scriptum_subscribe",
        arguments: { workspace_id: "ws", last_change_token: "tok-1" },
      });
      const readResult = await client.callTool({
        name: "scriptum_read",
        arguments: { workspace_id: "ws", doc_id: "doc", include_content: true },
      });
      const editResult = await client.callTool({
        name: "scriptum_edit",
        arguments: {
          workspace_id: "ws",
          doc_id: "doc",
          client_update_id: "cu-1",
          content_md: "hello",
        },
      });
      const listResult = await client.callTool({
        name: "scriptum_list",
        arguments: { workspace_id: "ws" },
      });
      const treeResult = await client.callTool({
        name: "scriptum_tree",
        arguments: { workspace_id: "ws", doc_id: "doc" },
      });
      const conflictsResult = await client.callTool({
        name: "scriptum_conflicts",
        arguments: { workspace_id: "ws" },
      });
      const historyResult = await client.callTool({
        name: "scriptum_history",
        arguments: { workspace_id: "ws", doc_id: "doc" },
      });
      const agentsResult = await client.callTool({
        name: "scriptum_agents",
        arguments: { workspace_id: "ws" },
      });
      const claimResult = await client.callTool({
        name: "scriptum_claim",
        arguments: {
          workspace_id: "ws",
          doc_id: "doc",
          section_id: "sec-1",
          ttl_sec: 300,
          mode: "shared",
        },
      });
      const bundleResult = await client.callTool({
        name: "scriptum_bundle",
        arguments: {
          workspace_id: "ws",
          doc_id: "doc",
          include: ["parents", "children"],
          token_budget: 2048,
        },
      });

      expect(calls.map((call) => call.method)).toEqual([
        MCP_TO_DAEMON_CONTRACT.scriptum_status,
        MCP_TO_DAEMON_CONTRACT.scriptum_subscribe,
        MCP_TO_DAEMON_CONTRACT.scriptum_read,
        MCP_TO_DAEMON_CONTRACT.scriptum_edit,
        MCP_TO_DAEMON_CONTRACT.scriptum_list,
        MCP_TO_DAEMON_CONTRACT.scriptum_tree,
        MCP_TO_DAEMON_CONTRACT.scriptum_conflicts,
        MCP_TO_DAEMON_CONTRACT.scriptum_history,
        MCP_TO_DAEMON_CONTRACT.scriptum_agents,
        MCP_TO_DAEMON_CONTRACT.scriptum_claim,
        MCP_TO_DAEMON_CONTRACT.scriptum_bundle,
      ]);

      const statusPayload = readToolResultPayload(statusResult);
      expect(statusPayload.structuredContent).toEqual({
        active_sessions: [],
        change_token: "tok-1",
        echoed_params: {
          workspace_id: "ws",
          agent_name: "contract-checker",
        },
      });

      const subscribePayload = readToolResultPayload(subscribeResult);
      expect(subscribePayload.structuredContent).toEqual({
        changed: false,
        change_token: "tok-1",
        status: {
          active_sessions: [],
          change_token: "tok-1",
          echoed_params: {
            workspace_id: "ws",
            agent_name: "contract-checker",
          },
        },
      });

      for (const result of [
        readResult,
        editResult,
        listResult,
        treeResult,
        conflictsResult,
        historyResult,
        agentsResult,
        claimResult,
        bundleResult,
      ]) {
        const payload = readToolResultPayload(result);
        expect(payload.textPayload).toEqual(payload.structuredContent);
        expect(payload.structuredContent).toMatchObject({
          rpc_method: expect.any(String),
          echoed_params: expect.anything(),
        });
      }
    } finally {
      await client.close();
      await server.close();
    }
  });
});

function readToolResultPayload(result: unknown): {
  textPayload: unknown;
  structuredContent: unknown;
} {
  if (!result || typeof result !== "object") {
    throw new Error("Expected MCP tool result object");
  }

  const payload = result as {
    content?: Array<{ type: string; text?: string }>;
    structuredContent?: unknown;
  };

  const firstContent = payload.content?.at(0);
  if (!firstContent || firstContent.type !== "text" || !firstContent.text) {
    throw new Error("Expected first tool content item to be text");
  }

  return {
    textPayload: JSON.parse(firstContent.text) as unknown,
    structuredContent: payload.structuredContent,
  };
}
