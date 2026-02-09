import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";

import type { DaemonClient } from "../daemon-client.js";
import {
  type AgentNameResolver,
  makeToolResult,
  PASSTHROUGH_TOOL_INPUT_SCHEMA,
  toToolPayload,
} from "../shared.js";

export function registerStatusTool(
  server: McpServer,
  daemonClient: DaemonClient,
  resolveAgentName: AgentNameResolver,
): void {
  server.registerTool(
    "scriptum_status",
    {
      description:
        "Return agent status including identity, sync state, and change token via daemon agent.status.",
      inputSchema: PASSTHROUGH_TOOL_INPUT_SCHEMA,
    },
    async (toolArgs: unknown) => {
      const rpcParams = {
        ...toToolPayload(toolArgs),
        agent_name: resolveAgentName(),
      };
      const payload = await daemonClient.request("agent.status", rpcParams);

      return makeToolResult(payload);
    },
  );
}
