import {
  McpServer,
  ResourceTemplate,
} from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import type { Transport } from "@modelcontextprotocol/sdk/shared/transport.js";
import type { Implementation } from "@modelcontextprotocol/sdk/types.js";
import { createDaemonClient, type DaemonClient } from "./daemon-client";
import {
  PASSTHROUGH_TOOL_INPUT_SCHEMA,
  listWorkspaces,
  makeResourceResult,
  makeToolResult,
  parseResourceVariable,
  resolveWorkspaceForDocId,
  toToolPayload,
  type AgentListResponse,
  type AgentNameResolver,
  type ToolDefinition,
  type ToolPayload,
} from "./shared";

const DEFAULT_AGENT_NAME = "mcp-agent";
const SERVER_INFO: Implementation = {
  name: "scriptum-mcp-server",
  version: "0.0.0",
};

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
  {
    name: "scriptum_conflicts",
    rpcMethod: "agent.conflicts",
    description:
      "List active section conflicts via daemon agent.conflicts. Forwards all tool arguments as JSON-RPC params.",
  },
  {
    name: "scriptum_history",
    rpcMethod: "doc.diff",
    description:
      "Show document diff/history via daemon doc.diff. Forwards all tool arguments as JSON-RPC params.",
  },
  {
    name: "scriptum_agents",
    rpcMethod: "agent.list",
    description:
      "List active agents in the workspace via daemon agent.list. Forwards all tool arguments as JSON-RPC params.",
  },
  {
    name: "scriptum_claim",
    rpcMethod: "agent.claim",
    description:
      "Claim a section lease via daemon agent.claim. Forwards all tool arguments as JSON-RPC params.",
  },
  {
    name: "scriptum_bundle",
    rpcMethod: "doc.bundle",
    description:
      "Fetch a context bundle via daemon doc.bundle. Forwards all tool arguments as JSON-RPC params.",
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
  server.registerResource(
    "scriptum-workspace",
    "scriptum://workspace",
    {
      title: "Scriptum Workspaces",
      description: "List workspaces from daemon workspace.list.",
      mimeType: "application/json",
    },
    async (uri) => {
      const payload = await daemonClient.request("workspace.list", {});
      return makeResourceResult(uri, payload);
    },
  );

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
