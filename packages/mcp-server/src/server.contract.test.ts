import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { describe, expect, it } from "vitest";

import type { DaemonClient } from "./daemon-client";
import { createServer } from "./server";

const MCP_TO_DAEMON_CONTRACT: Record<string, string> = {
  scriptum_read: "doc.read",
  scriptum_edit: "doc.edit",
  scriptum_list: "doc.tree",
  scriptum_tree: "doc.sections",
};

describe("mcp tool contract", () => {
  it("exposes the exact tool set expected by the daemon RPC contract", async () => {
    const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
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
        "scriptum_edit",
        "scriptum_list",
        "scriptum_read",
        "scriptum_status",
        "scriptum_tree",
      ]);
    } finally {
      await client.close();
      await server.close();
    }
  });

  it("maps MCP passthrough tools to the expected daemon methods", async () => {
    const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
    const calls: Array<{ method: string; params: unknown }> = [];
    const daemonClient: DaemonClient = {
      request: async (method, params) => {
        calls.push({ method, params: params ?? null });
        return { ok: true };
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

      await client.callTool({
        name: "scriptum_read",
        arguments: { workspace_id: "ws", doc_id: "doc", include_content: true },
      });
      await client.callTool({
        name: "scriptum_edit",
        arguments: {
          workspace_id: "ws",
          doc_id: "doc",
          client_update_id: "cu-1",
          content_md: "hello",
        },
      });
      await client.callTool({
        name: "scriptum_list",
        arguments: { workspace_id: "ws" },
      });
      await client.callTool({
        name: "scriptum_tree",
        arguments: { workspace_id: "ws", doc_id: "doc" },
      });

      expect(calls.map((call) => call.method)).toEqual([
        MCP_TO_DAEMON_CONTRACT.scriptum_read,
        MCP_TO_DAEMON_CONTRACT.scriptum_edit,
        MCP_TO_DAEMON_CONTRACT.scriptum_list,
        MCP_TO_DAEMON_CONTRACT.scriptum_tree,
      ]);
    } finally {
      await client.close();
      await server.close();
    }
  });
});
