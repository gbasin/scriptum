import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { describe, expect, it } from "vitest";

import type { DaemonClient } from "./daemon-client.js";
import { createServer, resolveAgentNameFromClientInfo } from "./server.js";

describe("mcp server scaffold", () => {
  it("falls back to mcp-agent when client info has no name", () => {
    expect(resolveAgentNameFromClientInfo(undefined)).toBe("mcp-agent");
    expect(resolveAgentNameFromClientInfo({ name: "  " })).toBe("mcp-agent");
    expect(resolveAgentNameFromClientInfo({ name: "cursor" })).toBe("cursor");
  });

  it("registers tool and resource handlers over MCP transport", async () => {
    const [clientTransport, serverTransport] =
      InMemoryTransport.createLinkedPair();
    const daemonCalls: Array<{ method: string; params: unknown }> = [];
    const workspaceListPayload = {
      items: [
        {
          workspace_id: "ws-1",
          name: "Workspace 1",
          root_path: "/tmp/ws-1",
          doc_count: 1,
        },
      ],
      total: 1,
    };
    const docTreePayload = {
      items: [{ doc_id: "doc-1", path: "README.md", title: "README" }],
      total: 1,
    };
    const docReadPayload = {
      metadata: {
        workspace_id: "ws-1",
        doc_id: "doc-1",
        path: "README.md",
      },
      sections: [],
      content_md: "# Scriptum",
      degraded: false,
    };
    const docSectionsPayload = {
      doc_id: "doc-1",
      sections: [
        {
          id: "s1",
          heading: "Overview",
          level: 1,
          start_line: 1,
          end_line: 3,
        },
      ],
    };
    const agentListPayload = {
      items: [
        {
          agent_id: "cursor",
          last_seen_at: "2026-02-07T00:00:00Z",
          active_sections: 2,
        },
      ],
    };
    const daemonClient: DaemonClient = {
      async request(method: string, params?: unknown) {
        daemonCalls.push({ method, params });
        if (method === "workspace.list") {
          return workspaceListPayload as never;
        }
        if (method === "doc.tree") {
          return docTreePayload as never;
        }
        if (method === "doc.read") {
          return docReadPayload as never;
        }
        if (method === "doc.sections") {
          return docSectionsPayload as never;
        }
        if (method === "agent.list") {
          return agentListPayload as never;
        }
        return {
          forwarded_method: method,
          forwarded_params: params ?? null,
        } as never;
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
          (resource) => resource.uri === "scriptum://workspace",
        ),
      ).toBe(true);
      expect(
        resourceList.resources.some(
          (resource) => resource.uri === "scriptum://agents",
        ),
      ).toBe(true);
      const resourceTemplates = await client.listResourceTemplates();
      expect(
        resourceTemplates.resourceTemplates.some(
          (resource) => resource.uriTemplate === "scriptum://docs/{id}",
        ),
      ).toBe(true);
      expect(
        resourceTemplates.resourceTemplates.some(
          (resource) =>
            resource.uriTemplate === "scriptum://docs/{id}/sections",
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
      const readPayload = readToolTextPayload(readToolResult);
      expect(readPayload).toEqual(docReadPayload);

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
        forwarded_params: Record<string, unknown>;
      };
      expect(editPayload.forwarded_method).toBe("doc.edit");
      expect(editPayload.forwarded_params).toEqual({
        workspace_id: "ws-1",
        doc_id: "doc-1",
        content: "Updated content",
        agent: "cursor",
        agent_id: "cursor",
      });

      const listToolResult = await client.callTool({
        name: "scriptum_list",
        arguments: { workspace_id: "ws-1" },
      });
      const listPayload = readToolTextPayload(listToolResult);
      expect(listPayload).toEqual(docTreePayload);

      const treeToolResult = await client.callTool({
        name: "scriptum_tree",
        arguments: {
          workspace_id: "ws-1",
          doc_id: "doc-1",
        },
      });
      const treePayload = readToolTextPayload(treeToolResult);
      expect(treePayload).toEqual(docSectionsPayload);

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
      const agentsToolPayload = readToolTextPayload(agentsToolResult);
      expect(agentsToolPayload).toEqual(agentListPayload);

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
        forwarded_params: Record<string, unknown>;
      };
      expect(claimPayload.forwarded_method).toBe("agent.claim");
      expect(claimPayload.forwarded_params).toEqual({
        workspace_id: "ws-1",
        doc_id: "doc-1",
        section_id: "sec-1",
        ttl_sec: 600,
        mode: "shared",
        note: "rewriting auth",
        agent_id: "cursor",
      });

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
            agent_id: "cursor",
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
            agent_id: "cursor",
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

      daemonCalls.length = 0;

      const workspaceResourceResult = await client.readResource({
        uri: "scriptum://workspace",
      });
      const workspacePayload = readResourceTextPayload(workspaceResourceResult);
      expect(workspacePayload).toEqual(workspaceListPayload);

      const agentsResourceResult = await client.readResource({
        uri: "scriptum://agents",
      });
      const agentsResourcePayload = readResourceTextPayload(
        agentsResourceResult,
      ) as {
        connected_agent: string;
        total_agents: number;
        workspaces: Array<{
          workspace_id: string;
          name: string;
          root_path: string;
          agents: unknown[];
        }>;
      };
      expect(agentsResourcePayload).toEqual({
        connected_agent: "cursor",
        total_agents: 1,
        workspaces: [
          {
            workspace_id: "ws-1",
            name: "Workspace 1",
            root_path: "/tmp/ws-1",
            agents: agentListPayload.items,
          },
        ],
      });

      const docResourceResult = await client.readResource({
        uri: "scriptum://docs/doc-1",
      });
      const docResourcePayload = readResourceTextPayload(docResourceResult);
      expect(docResourcePayload).toEqual(docReadPayload);

      const docSectionsResourceResult = await client.readResource({
        uri: "scriptum://docs/doc-1/sections",
      });
      const docSectionsResourcePayload = readResourceTextPayload(
        docSectionsResourceResult,
      );
      expect(docSectionsResourcePayload).toEqual(docSectionsPayload);

      expect(daemonCalls).toEqual([
        {
          method: "workspace.list",
          params: {},
        },
        {
          method: "workspace.list",
          params: {},
        },
        {
          method: "agent.list",
          params: {
            workspace_id: "ws-1",
          },
        },
        {
          method: "workspace.list",
          params: {},
        },
        {
          method: "doc.tree",
          params: {
            workspace_id: "ws-1",
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
          method: "workspace.list",
          params: {},
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

function readResourceTextPayload(result: unknown): unknown {
  if (!result || typeof result !== "object" || !("contents" in result)) {
    throw new Error("Expected contents in resource response");
  }
  const payload = result as {
    contents: Array<{ text?: string }>;
  };
  const firstContent = payload.contents.at(0);
  if (!firstContent || typeof firstContent.text !== "string") {
    throw new Error("Expected first resource content item to contain text");
  }
  return JSON.parse(firstContent.text) as unknown;
}
