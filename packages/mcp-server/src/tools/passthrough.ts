import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";

import type { DaemonClient } from "../daemon-client.js";
import {
  type AgentNameResolver,
  makeToolResult,
  PASSTHROUGH_TOOL_INPUT_SCHEMA,
  type ToolDefinition,
  type ToolPayload,
  toToolPayload,
} from "../shared.js";

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

const MUTATING_RPC_METHODS = new Set<string>(["doc.edit", "agent.claim"]);

export function registerPassthroughTools(
  server: McpServer,
  daemonClient: DaemonClient,
  resolveAgentName: AgentNameResolver,
): void {
  for (const definition of PASSTHROUGH_TOOL_DEFINITIONS) {
    server.registerTool(
      definition.name,
      {
        description: definition.description,
        inputSchema: PASSTHROUGH_TOOL_INPUT_SCHEMA,
      },
      async (toolArgs: unknown) => {
        const rpcParams = injectAgentIdForMutatingCalls(
          definition.rpcMethod,
          toToolPayload(toolArgs),
          resolveAgentName,
        );
        const payload = await daemonClient.request(
          definition.rpcMethod,
          rpcParams,
        );

        return makeToolResult(payload);
      },
    );
  }
}

function injectAgentIdForMutatingCalls(
  rpcMethod: string,
  rpcParams: ToolPayload,
  resolveAgentName: AgentNameResolver,
): ToolPayload {
  if (!MUTATING_RPC_METHODS.has(rpcMethod)) {
    return rpcParams;
  }

  return {
    ...rpcParams,
    agent_id: resolveAgentName(),
  };
}
