import {
  McpServer,
  ResourceTemplate,
} from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import type { Transport } from "@modelcontextprotocol/sdk/shared/transport.js";
import type { Implementation } from "@modelcontextprotocol/sdk/types.js";
import { createDaemonClient, type DaemonClient } from "./daemon-client";
import { registerAgentsResource } from "./resources/agents";
import { registerWorkspaceResource } from "./resources/workspace";
import {
  type AgentNameResolver,
  makeResourceResult,
  parseResourceVariable,
  resolveWorkspaceForDocId,
} from "./shared";
import { registerPassthroughTools } from "./tools/passthrough";
import { registerStatusTool } from "./tools/status";
import { registerSubscribeTool } from "./tools/subscribe";

const DEFAULT_AGENT_NAME = "mcp-agent";
const SERVER_INFO: Implementation = {
  name: "scriptum-mcp-server",
  version: "0.0.0",
};

export interface ScriptumMcpServer {
  start(): Promise<void>;
  close(): Promise<void>;
  resolveAgentName(): string;
}

export interface ScriptumMcpServerOptions {
  readonly daemonClient?: DaemonClient;
  readonly transportFactory?: () => Transport;
}

class DefaultScriptumMcpServer implements ScriptumMcpServer {
  private readonly daemonClient: DaemonClient;
  private readonly mcpServer: McpServer;
  private readonly transportFactory: () => Transport;
  private started = false;

  constructor(options: ScriptumMcpServerOptions = {}) {
    this.mcpServer = new McpServer(SERVER_INFO);
    this.daemonClient = options.daemonClient ?? createDaemonClient();
    this.transportFactory = options.transportFactory ?? createStdioTransport;

    registerToolHandlers(this.mcpServer, this.daemonClient, () =>
      this.resolveAgentName(),
    );
    registerResourceHandlers(this.mcpServer, this.daemonClient, () =>
      this.resolveAgentName(),
    );
  }

  async start(): Promise<void> {
    if (this.started) {
      return;
    }

    await this.mcpServer.connect(this.transportFactory());
    this.started = true;
  }

  async close(): Promise<void> {
    if (!this.started) {
      return;
    }

    await this.mcpServer.close();
    this.started = false;
  }

  resolveAgentName(): string {
    return resolveAgentNameFromClientInfo(
      this.mcpServer.server.getClientVersion(),
    );
  }
}

export function createServer(
  options: ScriptumMcpServerOptions = {},
): ScriptumMcpServer {
  return new DefaultScriptumMcpServer(options);
}

export function createStdioTransport(): Transport {
  return new StdioServerTransport();
}

export function resolveAgentNameFromClientInfo(
  clientInfo: Pick<Implementation, "name"> | undefined,
): string {
  const trimmed = clientInfo?.name?.trim();
  return trimmed && trimmed.length > 0 ? trimmed : DEFAULT_AGENT_NAME;
}

function registerToolHandlers(
  server: McpServer,
  daemonClient: DaemonClient,
  resolveAgentName: AgentNameResolver,
): void {
  registerStatusTool(server, daemonClient, resolveAgentName);
  registerSubscribeTool(server, daemonClient, resolveAgentName);
  registerPassthroughTools(server, daemonClient);
}

function registerResourceHandlers(
  server: McpServer,
  daemonClient: DaemonClient,
  resolveAgentName: AgentNameResolver,
): void {
  registerWorkspaceResource(server, daemonClient);
  registerAgentsResource(server, daemonClient, resolveAgentName);

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
