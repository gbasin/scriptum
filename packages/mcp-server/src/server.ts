import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import type { Transport } from "@modelcontextprotocol/sdk/shared/transport.js";
import { CallToolRequestParamsSchema } from "@modelcontextprotocol/sdk/types.js";
import type { Implementation } from "@modelcontextprotocol/sdk/types.js";
import { createDaemonClient, type DaemonClient } from "./daemon-client";

const DEFAULT_AGENT_NAME = "mcp-agent";
const DEFAULT_STATUS_CHANGE_TOKEN = "bootstrap";
const SERVER_INFO: Implementation = {
  name: "scriptum-mcp-server",
  version: "0.0.0",
};
const PASSTHROUGH_TOOL_INPUT_SCHEMA = CallToolRequestParamsSchema.shape.arguments;

interface ToolDefinition {
  readonly description: string;
  readonly name: string;
  readonly rpcMethod: string;
}

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
];

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

    registerToolHandlers(
      this.mcpServer,
      this.daemonClient,
      () => this.resolveAgentName(),
    );
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
type ToolPayload = Record<string, unknown>;

function registerToolHandlers(
  server: McpServer,
  daemonClient: DaemonClient,
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

      return makeToolResult(payload);
    },
  );

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

function toToolPayload(value: unknown): ToolPayload {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return {};
  }
  return value as ToolPayload;
}

function makeToolResult(payload: unknown) {
  const normalizedPayload = payload ?? null;
  return {
    content: [{ type: "text", text: JSON.stringify(normalizedPayload) }],
    structuredContent: normalizedPayload,
  } as const;
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
