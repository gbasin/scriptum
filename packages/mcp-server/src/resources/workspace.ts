import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";

import type { DaemonClient } from "../daemon-client.js";
import { listWorkspaces, makeResourceResult } from "../shared.js";

export function registerWorkspaceResource(
  server: McpServer,
  daemonClient: DaemonClient,
): void {
  server.registerResource(
    "scriptum-workspace",
    "scriptum://workspace",
    {
      title: "Scriptum Workspaces",
      description: "List workspaces from daemon workspace.list.",
      mimeType: "application/json",
    },
    async (uri) => {
      const items = await listWorkspaces(daemonClient);
      return makeResourceResult(uri, {
        items,
        total: items.length,
      });
    },
  );
}
