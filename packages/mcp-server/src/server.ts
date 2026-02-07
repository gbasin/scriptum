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
  makeToolResult,
  PASSTHROUGH_TOOL_INPUT_SCHEMA,
  parseResourceVariable,
  resolveWorkspaceForDocId,
  type ToolPayload,
  toToolPayload,
} from "./shared";
import { registerPassthroughTools } from "./tools/passthrough";

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
  server.registerTool(
    "scriptum_status",
    {
      description:
        "Return agent status including identity, sync state, and change token via daemon agent.status.",
      inputSchema: PASSTHROUGH_TOOL_INPUT_SCHEMA,
    },
    async (toolArgs) => {
      const rpcParams = {
        ...toToolPayload(toolArgs),
        agent_name: resolveAgentName(),
      };
      const payload = await daemonClient.request("agent.status", rpcParams);

      return makeToolResult(payload);
    },
  );

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

  registerPassthroughTools(server, daemonClient);
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
