import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { describe, expect, it } from "vitest";

import type { DaemonClient } from "./daemon-client";
import { createServer, resolveAgentNameFromClientInfo } from "./server";

describe("mcp server scaffold", () => {
  it("falls back to mcp-agent when client info has no name", () => {
    expect(resolveAgentNameFromClientInfo(undefined)).toBe("mcp-agent");
    expect(resolveAgentNameFromClientInfo({ name: "  " })).toBe("mcp-agent");
    expect(resolveAgentNameFromClientInfo({ name: "cursor" })).toBe("cursor");
  });

  it("registers tool and resource handlers over MCP transport", async () => {
    const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
    const daemonCalls: Array<{ method: string; params: unknown }> = [];
    const daemonClient: DaemonClient = {
      request: async (method, params) => {
        daemonCalls.push({ method, params });
        return {
          forwarded_method: method,
          forwarded_params: params ?? null,
        };
      },
    };
    const server = createServer({
      daemonClient,
      transportFactory: () => serverTransport,
    });
    const client = new Client({
      name: "cursor",
      version: "1.0.0",
    });

    await server.start();

    try {
      await client.connect(clientTransport);

      const toolList = await client.listTools();
      const toolNames = toolList.tools.map((tool) => tool.name).sort();
      expect(toolNames).toContain("scriptum_status");
      expect(toolNames).toContain("scriptum_read");
      expect(toolNames).toContain("scriptum_edit");
      expect(toolNames).toContain("scriptum_list");
      expect(toolNames).toContain("scriptum_tree");
      expect(toolNames).toContain("scriptum_conflicts");
      expect(toolNames).toContain("scriptum_history");
      expect(toolNames).toContain("scriptum_agents");
      expect(toolNames).toContain("scriptum_subscribe");
      expect(toolNames).toContain("scriptum_claim");
      expect(toolNames).toContain("scriptum_bundle");

      const resourceList = await client.listResources();
      expect(
        resourceList.resources.some(
          (resource) => resource.uri === "scriptum://agents",
        ),
      ).toBe(true);

      const toolResult = await client.callTool({
        name: "scriptum_status",
        arguments: {
          workspace_id: "ws-1",
        },
      });
      const statusPayload = readToolTextPayload(toolResult) as {
        forwarded_method: string;
        forwarded_params: Record<string, unknown>;
      };
      expect(statusPayload.forwarded_method).toBe("agent.status");
      expect(statusPayload.forwarded_params.agent_name).toBe("cursor");
      expect(statusPayload.forwarded_params.workspace_id).toBe("ws-1");

      const subscribeToolResult = await client.callTool({
        name: "scriptum_subscribe",
        arguments: {
          workspace_id: "ws-1",
          last_change_token: "token-1",
        },
      });
      const subscribePayload = readToolTextPayload(subscribeToolResult) as {
        changed: boolean;
        change_token: string | null;
        status: {
          forwarded_method: string;
          forwarded_params: Record<string, unknown>;
        };
      };
      expect(subscribePayload.changed).toBe(true);
      expect(subscribePayload.change_token).toBeNull();
      expect(subscribePayload.status.forwarded_method).toBe("agent.status");
      expect(subscribePayload.status.forwarded_params).toEqual({
        workspace_id: "ws-1",
        agent_name: "cursor",
      });

      const readToolResult = await client.callTool({
        name: "scriptum_read",
        arguments: {
          workspace_id: "ws-1",
          doc_id: "doc-1",
          include_content: true,
        },
      });
      const readPayload = readToolTextPayload(readToolResult) as {
        forwarded_method: string;
        forwarded_params: Record<string, unknown>;
      };
      expect(readPayload.forwarded_method).toBe("doc.read");
      expect(readPayload.forwarded_params).toEqual({
        workspace_id: "ws-1",
        doc_id: "doc-1",
        include_content: true,
      });

      const editToolResult = await client.callTool({
        name: "scriptum_edit",
        arguments: {
          workspace_id: "ws-1",
          doc_id: "doc-1",
          content: "Updated content",
          agent: "cursor",
        },
      });
      const editPayload = readToolTextPayload(editToolResult) as {
        forwarded_method: string;
      };
      expect(editPayload.forwarded_method).toBe("doc.edit");

      const listToolResult = await client.callTool({
        name: "scriptum_list",
        arguments: { workspace_id: "ws-1" },
      });
      const listPayload = readToolTextPayload(listToolResult) as {
        forwarded_method: string;
      };
      expect(listPayload.forwarded_method).toBe("doc.tree");

      const treeToolResult = await client.callTool({
        name: "scriptum_tree",
        arguments: {
          workspace_id: "ws-1",
          doc_id: "doc-1",
        },
      });
      const treePayload = readToolTextPayload(treeToolResult) as {
        forwarded_method: string;
      };
      expect(treePayload.forwarded_method).toBe("doc.sections");

      const conflictsToolResult = await client.callTool({
        name: "scriptum_conflicts",
        arguments: { workspace_id: "ws-1" },
      });
      const conflictsPayload = readToolTextPayload(conflictsToolResult) as {
        forwarded_method: string;
      };
      expect(conflictsPayload.forwarded_method).toBe("agent.conflicts");

      const historyToolResult = await client.callTool({
        name: "scriptum_history",
        arguments: { workspace_id: "ws-1", doc_id: "doc-1", from_seq: 0 },
      });
      const historyPayload = readToolTextPayload(historyToolResult) as {
        forwarded_method: string;
      };
      expect(historyPayload.forwarded_method).toBe("doc.diff");

      const agentsToolResult = await client.callTool({
        name: "scriptum_agents",
        arguments: { workspace_id: "ws-1" },
      });
      const agentsToolPayload = readToolTextPayload(agentsToolResult) as {
        forwarded_method: string;
      };
      expect(agentsToolPayload.forwarded_method).toBe("agent.list");

      const claimToolResult = await client.callTool({
        name: "scriptum_claim",
        arguments: {
          workspace_id: "ws-1",
          doc_id: "doc-1",
          section_id: "sec-1",
          ttl_sec: 600,
          mode: "shared",
          note: "rewriting auth",
        },
      });
      const claimPayload = readToolTextPayload(claimToolResult) as {
        forwarded_method: string;
      };
      expect(claimPayload.forwarded_method).toBe("agent.claim");

      const bundleToolResult = await client.callTool({
        name: "scriptum_bundle",
        arguments: {
          workspace_id: "ws-1",
          doc_id: "doc-1",
          section_id: "sec-1",
          include: ["parents", "children", "backlinks"],
          token_budget: 2048,
        },
      });
      const bundlePayload = readToolTextPayload(bundleToolResult) as {
        forwarded_method: string;
      };
      expect(bundlePayload.forwarded_method).toBe("doc.bundle");

      expect(daemonCalls).toEqual([
        {
          method: "agent.status",
          params: {
            workspace_id: "ws-1",
            agent_name: "cursor",
          },
        },
        {
          method: "agent.status",
          params: {
            workspace_id: "ws-1",
            agent_name: "cursor",
          },
        },
        {
          method: "doc.read",
          params: {
            workspace_id: "ws-1",
            doc_id: "doc-1",
            include_content: true,
          },
        },
        {
          method: "doc.edit",
          params: {
            workspace_id: "ws-1",
            doc_id: "doc-1",
            content: "Updated content",
            agent: "cursor",
          },
        },
        {
          method: "doc.tree",
          params: {
            workspace_id: "ws-1",
          },
        },
        {
          method: "doc.sections",
          params: {
            workspace_id: "ws-1",
            doc_id: "doc-1",
          },
        },
        {
          method: "agent.conflicts",
          params: {
            workspace_id: "ws-1",
          },
        },
        {
          method: "doc.diff",
          params: {
            workspace_id: "ws-1",
            doc_id: "doc-1",
            from_seq: 0,
          },
        },
        {
          method: "agent.list",
          params: {
            workspace_id: "ws-1",
          },
        },
        {
          method: "agent.claim",
          params: {
            workspace_id: "ws-1",
            doc_id: "doc-1",
            section_id: "sec-1",
            ttl_sec: 600,
            mode: "shared",
            note: "rewriting auth",
          },
        },
        {
          method: "doc.bundle",
          params: {
            workspace_id: "ws-1",
            doc_id: "doc-1",
            section_id: "sec-1",
            include: ["parents", "children", "backlinks"],
            token_budget: 2048,
          },
        },
      ]);

      const resourceResult = await client.readResource({ uri: "scriptum://agents" });
      const firstResource = resourceResult.contents.at(0);
      if (!firstResource || !("text" in firstResource)) {
        throw new Error("Expected text resource content for scriptum://agents");
      }
      const agentsPayload = JSON.parse(firstResource.text) as {
        agents: Array<{ name: string }>;
      };
      expect(agentsPayload).toEqual({
        agents: [{ name: "cursor" }],
      });
    } finally {
      await client.close();
      await server.close();
    }
  });
});

function readToolTextPayload(result: unknown): unknown {
  if (!result || typeof result !== "object" || !("content" in result)) {
    throw new Error("Expected content in tool response");
  }
  const payload = result as {
    content: Array<{ type: string; text?: string }>;
  };
  const firstContent = payload.content.at(0);
  if (!firstContent || firstContent.type !== "text" || !firstContent.text) {
    throw new Error("Expected first tool content item to be text");
  }
  return JSON.parse(firstContent.text) as unknown;
}
