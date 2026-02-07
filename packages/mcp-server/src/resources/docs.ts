import {
  ResourceTemplate,
  type McpServer,
} from "@modelcontextprotocol/sdk/server/mcp.js";

import type { DaemonClient } from "../daemon-client";
import {
  makeResourceResult,
  parseResourceVariable,
  resolveWorkspaceForDocId,
} from "../shared";

export function registerDocResources(
  server: McpServer,
  daemonClient: DaemonClient,
): void {
  server.registerResource(
    "scriptum-doc-sections",
    new ResourceTemplate("scriptum://docs/{id}/sections", { list: undefined }),
    {
      title: "Scriptum Document Sections",
      description: "Read section tree for a document by ID.",
      mimeType: "application/json",
    },
    async (uri, variables) => {
      const docId = parseResourceVariable(variables, "id");
      const workspace = await resolveWorkspaceForDocId(daemonClient, docId);
      if (!workspace) {
        throw new Error(`document ${docId} not found in any workspace`);
      }

      const payload = await daemonClient.request("doc.sections", {
        workspace_id: workspace.workspace_id,
        doc_id: docId,
      });
      return makeResourceResult(uri, payload);
    },
  );

  server.registerResource(
    "scriptum-doc",
    new ResourceTemplate("scriptum://docs/{id}", { list: undefined }),
    {
      title: "Scriptum Document Content",
      description: "Read markdown document content by document ID.",
      mimeType: "application/json",
    },
    async (uri, variables) => {
      const docId = parseResourceVariable(variables, "id");
      const workspace = await resolveWorkspaceForDocId(daemonClient, docId);
      if (!workspace) {
        throw new Error(`document ${docId} not found in any workspace`);
      }

      const payload = await daemonClient.request("doc.read", {
        workspace_id: workspace.workspace_id,
        doc_id: docId,
        include_content: true,
      });
      return makeResourceResult(uri, payload);
    },
  );
}
