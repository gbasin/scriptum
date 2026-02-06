import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { describe, expect, it } from "vitest";

import { createServer, resolveAgentNameFromClientInfo } from "./server";

describe("mcp server scaffold", () => {
  it("falls back to mcp-agent when client info has no name", () => {
    expect(resolveAgentNameFromClientInfo(undefined)).toBe("mcp-agent");
    expect(resolveAgentNameFromClientInfo({ name: "  " })).toBe("mcp-agent");
    expect(resolveAgentNameFromClientInfo({ name: "cursor" })).toBe("cursor");
  });

  it("registers tool and resource handlers over MCP transport", async () => {
    const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
    const server = createServer({
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
