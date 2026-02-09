import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import type { Transport } from "@modelcontextprotocol/sdk/shared/transport.js";
import type { Implementation } from "@modelcontextprotocol/sdk/types.js";
import { createDaemonClient, type DaemonClient } from "./daemon-client.js";
import { registerAgentsResource } from "./resources/agents.js";
import { registerDocResources } from "./resources/docs.js";
import { registerWorkspaceResource } from "./resources/workspace.js";
import type { AgentNameResolver } from "./shared.js";
import { registerPassthroughTools } from "./tools/passthrough.js";
import { registerStatusTool } from "./tools/status.js";
import { registerSubscribeTool } from "./tools/subscribe.js";

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
  registerPassthroughTools(server, daemonClient, resolveAgentName);
}

function registerResourceHandlers(
  server: McpServer,
  daemonClient: DaemonClient,
  resolveAgentName: AgentNameResolver,
): void {
  registerWorkspaceResource(server, daemonClient);
  registerAgentsResource(server, daemonClient, resolveAgentName);
  registerDocResources(server, daemonClient);
}
