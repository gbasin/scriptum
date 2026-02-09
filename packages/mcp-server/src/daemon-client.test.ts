import { rm } from "node:fs/promises";
import { createServer, type Server as NetServer } from "node:net";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import { createDaemonClient, DaemonNotRunningError } from "./daemon-client";

describe("createDaemonClient", () => {
  it("calls daemon JSON-RPC over unix socket", async () => {
    if (process.platform === "win32") {
      return;
    }

    const socketPath = uniqueSocketPath("json-rpc");
    await cleanupSocketFile(socketPath);
    let receivedMethod = "";
    let receivedProtocolVersion = "";
    let receivedParams: unknown = null;

    const server = createServer((socket) => {
      let requestBuffer = "";
      socket.on("data", (chunk: Buffer) => {
        requestBuffer += chunk.toString("utf8");
        const newlineIndex = requestBuffer.indexOf("\n");
        if (newlineIndex === -1) {
          return;
        }

        const line = requestBuffer.slice(0, newlineIndex);
        const request = JSON.parse(line) as {
          protocol_version: string;
          method: string;
          params?: unknown;
        };
        receivedProtocolVersion = request.protocol_version;
        receivedMethod = request.method;
        receivedParams = request.params ?? null;

        socket.write(
          `${JSON.stringify({
            jsonrpc: "2.0",
            id: 1,
            result: { agent_id: "claude-1" },
          })}\n`,
        );
      });
    });

    try {
      await listenOnSocket(server, socketPath);
    } catch (error) {
      if (isSocketPermissionError(error)) {
        return;
      }
      throw error;
    }

    try {
      const client = createDaemonClient({ socketPath, timeoutMs: 1_000 });
      const result = await client.request<{ agent_id: string }>(
        "agent.whoami",
        {},
      );

      expect(result.agent_id).toBe("claude-1");
      expect(receivedMethod).toBe("agent.whoami");
      expect(receivedProtocolVersion).toBe("scriptum-rpc.v1");
      expect(receivedParams).toEqual({});
    } finally {
      await closeServer(server);
      await cleanupSocketFile(socketPath);
    }
  });

  it("throws DaemonNotRunningError when daemon socket is unavailable", async () => {
    if (process.platform === "win32") {
      return;
    }

    const missingSocketPath = uniqueSocketPath("missing");
    await cleanupSocketFile(missingSocketPath);

    const client = createDaemonClient({
      socketPath: missingSocketPath,
      timeoutMs: 500,
    });

    await expect(client.request("workspace.list", {})).rejects.toBeInstanceOf(
      DaemonNotRunningError,
    );
  });
});

function uniqueSocketPath(prefix: string): string {
  return join(
    "/tmp",
    `smcp-${prefix}-${process.pid}-${Date.now().toString(36)}-${Math.random()
      .toString(16)
      .slice(2, 8)}.sock`,
  );
}

async function cleanupSocketFile(socketPath: string): Promise<void> {
  await rm(socketPath, { force: true });
}

async function listenOnSocket(
  server: NetServer,
  socketPath: string,
): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(socketPath, () => resolve());
  });
}

async function closeServer(server: NetServer): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    server.close((error) => {
      if (error) {
        reject(error);
        return;
      }
      resolve();
    });
  });
}

function isSocketPermissionError(error: unknown): boolean {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: string }).code === "EPERM"
  );
}
