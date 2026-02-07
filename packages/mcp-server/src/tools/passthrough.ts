import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";

import type { DaemonClient } from "../daemon-client";
import {
  makeToolResult,
  PASSTHROUGH_TOOL_INPUT_SCHEMA,
  type ToolDefinition,
  toToolPayload,
} from "../shared";

const PASSTHROUGH_TOOL_DEFINITIONS: readonly ToolDefinition[] = [
  {
    name: "scriptum_read",
    rpcMethod: "doc.read",
    description:
      "Read document content via daemon doc.read. Forwards all tool arguments as JSON-RPC params.",
  },
  {
    name: "scriptum_edit",
    rpcMethod: "doc.edit",
    description:
      "Edit document content via daemon doc.edit. Forwards all tool arguments as JSON-RPC params.",
  },
  {
    name: "scriptum_list",
    rpcMethod: "doc.tree",
    description:
      "List workspace documents via daemon doc.tree. Forwards all tool arguments as JSON-RPC params.",
  },
  {
    name: "scriptum_tree",
    rpcMethod: "doc.sections",
    description:
      "List document section structure via daemon doc.sections. Forwards all tool arguments as JSON-RPC params.",
  },
  {
    name: "scriptum_conflicts",
    rpcMethod: "agent.conflicts",
    description:
      "List active section conflicts via daemon agent.conflicts. Forwards all tool arguments as JSON-RPC params.",
  },
  {
    name: "scriptum_history",
    rpcMethod: "doc.diff",
    description:
      "Show document diff/history via daemon doc.diff. Forwards all tool arguments as JSON-RPC params.",
  },
  {
    name: "scriptum_agents",
    rpcMethod: "agent.list",
    description:
      "List active agents in the workspace via daemon agent.list. Forwards all tool arguments as JSON-RPC params.",
  },
  {
    name: "scriptum_claim",
    rpcMethod: "agent.claim",
    description:
      "Claim a section lease via daemon agent.claim. Forwards all tool arguments as JSON-RPC params.",
  },
  {
    name: "scriptum_bundle",
    rpcMethod: "doc.bundle",
    description:
      "Fetch a context bundle via daemon doc.bundle. Forwards all tool arguments as JSON-RPC params.",
  },
];

export function registerPassthroughTools(
  server: McpServer,
  daemonClient: DaemonClient,
): void {
  for (const definition of PASSTHROUGH_TOOL_DEFINITIONS) {
    server.registerTool(
      definition.name,
      {
        description: definition.description,
        inputSchema: PASSTHROUGH_TOOL_INPUT_SCHEMA,
      },
      async (toolArgs) => {
        const rpcParams = toToolPayload(toolArgs);
        const payload = await daemonClient.request(
          definition.rpcMethod,
          rpcParams,
        );

        return makeToolResult(payload);
      },
    );
  }
}
