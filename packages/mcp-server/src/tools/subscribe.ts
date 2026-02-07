import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";

import type { DaemonClient } from "../daemon-client";
import {
  type AgentNameResolver,
  makeToolResult,
  PASSTHROUGH_TOOL_INPUT_SCHEMA,
  type ToolPayload,
  toToolPayload,
} from "../shared";

export function registerSubscribeTool(
  server: McpServer,
  daemonClient: DaemonClient,
  resolveAgentName: AgentNameResolver,
): void {
  server.registerTool(
    "scriptum_subscribe",
    {
      description:
        "Polling subscribe helper. Calls daemon agent.status, compares change token, and reports whether it changed.",
      inputSchema: PASSTHROUGH_TOOL_INPUT_SCHEMA,
    },
    async (toolArgs) => {
      const subscribeParams = toToolPayload(toolArgs);
      const previousChangeToken = extractPreviousChangeToken(subscribeParams);
      const statusPayload = await daemonClient.request("agent.status", {
        ...stripSubscribeTokenParams(subscribeParams),
        agent_name: resolveAgentName(),
      });
      const currentChangeToken = extractChangeToken(statusPayload);

      return makeToolResult({
        changed:
          previousChangeToken === null
            ? true
            : currentChangeToken !== previousChangeToken,
        change_token: currentChangeToken,
        status: statusPayload,
      });
    },
  );
}

function extractPreviousChangeToken(payload: ToolPayload): string | null {
  const direct = payload.change_token;
  if (typeof direct === "string" && direct.length > 0) {
    return direct;
  }

  const alias = payload.last_change_token;
  if (typeof alias === "string" && alias.length > 0) {
    return alias;
  }

  return null;
}

function stripSubscribeTokenParams(payload: ToolPayload): ToolPayload {
  const { change_token, last_change_token, ...rest } = payload;
  void change_token;
  void last_change_token;
  return rest;
}

function extractChangeToken(payload: unknown): string | null {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    return null;
  }

  const value = (payload as Record<string, unknown>).change_token;
  if (typeof value !== "string" || value.length === 0) {
    return null;
  }

  return value;
}
