import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import type { Transport } from "@modelcontextprotocol/sdk/shared/transport.js";
import type { Implementation } from "@modelcontextprotocol/sdk/types.js";

const DEFAULT_AGENT_NAME = "mcp-agent";
const DEFAULT_STATUS_CHANGE_TOKEN = "bootstrap";
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
  readonly transportFactory?: () => Transport;
}

class DefaultScriptumMcpServer implements ScriptumMcpServer {
  private readonly mcpServer: McpServer;
  private readonly transportFactory: () => Transport;
  private started = false;

  constructor(options: ScriptumMcpServerOptions = {}) {
    this.mcpServer = new McpServer(SERVER_INFO);
    this.transportFactory = options.transportFactory ?? createStdioTransport;

    registerToolHandlers(this.mcpServer, () => this.resolveAgentName());
    registerResourceHandlers(this.mcpServer, () => this.resolveAgentName());
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
    return resolveAgentNameFromClientInfo(this.mcpServer.server.getClientVersion());
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

type AgentNameResolver = () => string;

function registerToolHandlers(
  server: McpServer,
  resolveAgentName: AgentNameResolver,
): void {
  server.registerTool(
    "scriptum_status",
    {
      description:
        "Scaffold status tool. Returns agent identity and placeholder change token.",
      inputSchema: {},
    },
    async () => {
      const payload = {
        agent_name: resolveAgentName(),
        change_token: DEFAULT_STATUS_CHANGE_TOKEN,
      };

      return {
        content: [{ type: "text", text: JSON.stringify(payload) }],
        structuredContent: payload,
      };
    },
  );
}

function registerResourceHandlers(
  server: McpServer,
  resolveAgentName: AgentNameResolver,
): void {
  server.registerResource(
    "scriptum-agents",
    "scriptum://agents",
    {
      title: "Scriptum Agents",
      description:
        "Scaffold resource returning the connected MCP agent name.",
      mimeType: "application/json",
    },
    async (uri) => {
      const payload = {
        agents: [{ name: resolveAgentName() }],
      };

      return {
        contents: [
          {
            uri: uri.toString(),
            mimeType: "application/json",
            text: JSON.stringify(payload),
          },
        ],
      };
    },
  );
}
