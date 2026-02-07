import type { Variables } from "@modelcontextprotocol/sdk/shared/uriTemplate.js";
import { CallToolRequestParamsSchema } from "@modelcontextprotocol/sdk/types.js";

import type { DaemonClient } from "./daemon-client";

export const PASSTHROUGH_TOOL_INPUT_SCHEMA =
  CallToolRequestParamsSchema.shape.arguments;

export interface ToolDefinition {
  readonly description: string;
  readonly name: string;
  readonly rpcMethod: string;
}

export interface WorkspaceSummary {
  readonly workspace_id: string;
  readonly name: string;
  readonly root_path: string;
}

export interface WorkspaceListResponse {
  readonly items?: WorkspaceSummary[];
}

export interface DocTreeEntry {
  readonly doc_id: string;
}

export interface DocTreeResponse {
  readonly items?: DocTreeEntry[];
}

export interface AgentListResponse {
  readonly items?: unknown[];
}

export type AgentNameResolver = () => string;
export type ToolPayload = Record<string, unknown>;

export function toToolPayload(value: unknown): ToolPayload {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return {};
  }
  return value as ToolPayload;
}

export function makeToolResult(payload: unknown) {
  const text = JSON.stringify(payload ?? null);
  return {
    content: [{ type: "text" as const, text }],
    structuredContent:
      payload != null && typeof payload === "object" && !Array.isArray(payload)
        ? (payload as Record<string, unknown>)
        : undefined,
  };
}

export async function listWorkspaces(
  daemonClient: DaemonClient,
): Promise<WorkspaceSummary[]> {
  const response = await daemonClient.request<WorkspaceListResponse>(
    "workspace.list",
    {},
  );
  return response.items ?? [];
}

export async function resolveWorkspaceForDocId(
  daemonClient: DaemonClient,
  docId: string,
): Promise<WorkspaceSummary | undefined> {
  const workspaces = await listWorkspaces(daemonClient);
  for (const workspace of workspaces) {
    const tree = await daemonClient.request<DocTreeResponse>("doc.tree", {
      workspace_id: workspace.workspace_id,
    });
    const items = tree.items ?? [];
    if (items.some((item) => item.doc_id === docId)) {
      return workspace;
    }
  }

  return undefined;
}

export function parseResourceVariable(
  variables: Variables,
  key: string,
): string {
  const value = variables[key];
  const raw =
    typeof value === "string"
      ? value
      : Array.isArray(value)
        ? value[0]
        : undefined;
  if (!raw) {
    throw new Error(`resource URI is missing required variable: ${key}`);
  }

  const normalized = raw.trim();
  if (!normalized) {
    throw new Error(`resource URI variable ${key} must not be empty`);
  }

  return normalized;
}

export function makeResourceResult(uri: URL, payload: unknown) {
  const normalizedPayload = payload ?? null;
  return {
    contents: [
      {
        uri: uri.toString(),
        mimeType: "application/json",
        text: JSON.stringify(normalizedPayload),
      },
    ],
  };
}
