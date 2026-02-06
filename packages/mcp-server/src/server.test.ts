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
      expect(toolList.tools.some((tool) => tool.name === "scriptum_status")).toBe(
        true,
      );
      expect(toolList.tools.some((tool) => tool.name === "scriptum_read")).toBe(
        true,
      );
      expect(toolList.tools.some((tool) => tool.name === "scriptum_edit")).toBe(
        true,
      );
      expect(toolList.tools.some((tool) => tool.name === "scriptum_list")).toBe(
        true,
      );
      expect(toolList.tools.some((tool) => tool.name === "scriptum_tree")).toBe(
        true,
      );

      const resourceList = await client.listResources();
      expect(
        resourceList.resources.some(
          (resource) => resource.uri === "scriptum://agents",
        ),
      ).toBe(true);

      const toolResult = await client.callTool({
        name: "scriptum_status",
        arguments: {},
      });
      if (!("content" in toolResult)) {
        throw new Error("Expected content in scriptum_status response");
      }

      const firstContent = toolResult.content.at(0);
      if (!firstContent || firstContent.type !== "text") {
        throw new Error("Expected text content from scriptum_status response");
      }
      const statusPayload = JSON.parse(firstContent.text) as {
        agent_name: string;
        change_token: string;
      };
      expect(statusPayload.agent_name).toBe("cursor");
      expect(statusPayload.change_token).toBe("bootstrap");

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
      expect(daemonCalls).toEqual([
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
