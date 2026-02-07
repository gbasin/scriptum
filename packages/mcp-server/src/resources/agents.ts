import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";

import type { DaemonClient } from "../daemon-client";
import {
  type AgentListResponse,
  type AgentNameResolver,
  listWorkspaces,
  makeResourceResult,
} from "../shared";

export function registerAgentsResource(
  server: McpServer,
  daemonClient: DaemonClient,
  resolveAgentName: AgentNameResolver,
): void {
  server.registerResource(
    "scriptum-agents",
    "scriptum://agents",
    {
      title: "Scriptum Agents",
      description: "List active agents grouped by workspace.",
      mimeType: "application/json",
    },
    async (uri) => {
      const workspaces = await listWorkspaces(daemonClient);
      const workspacesWithAgents = [];
      for (const workspace of workspaces) {
        const agentList = await daemonClient.request<AgentListResponse>(
          "agent.list",
          {
            workspace_id: workspace.workspace_id,
          },
        );
        workspacesWithAgents.push({
          workspace_id: workspace.workspace_id,
          name: workspace.name,
          root_path: workspace.root_path,
          agents: agentList.items ?? [],
        });
      }

      const payload = {
        connected_agent: resolveAgentName(),
        workspaces: workspacesWithAgents,
        total_agents: workspacesWithAgents.reduce(
          (total, workspace) => total + workspace.agents.length,
          0,
        ),
      };

      return makeResourceResult(uri, payload);
    },
  );
}
