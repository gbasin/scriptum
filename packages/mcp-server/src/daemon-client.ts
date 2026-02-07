import { createConnection } from "node:net";
import { homedir } from "node:os";
import { join } from "node:path";

const DEFAULT_TIMEOUT_MS = 3_000;
const UNIX_SOCKET_RELATIVE_PATH = ".scriptum/daemon.sock";
const WINDOWS_NAMED_PIPE_PATH = "\\\\.\\pipe\\scriptum-daemon";

interface JsonRpcRequest {
  readonly jsonrpc: "2.0";
  readonly id: number;
  readonly method: string;
  readonly params?: unknown;
}

interface JsonRpcErrorPayload {
  readonly code: number;
  readonly message: string;
  readonly data?: unknown;
}

interface JsonRpcResponse<T> {
  readonly jsonrpc: string;
  readonly id: number | string | null;
  readonly result?: T;
  readonly error?: JsonRpcErrorPayload;
}

export interface DaemonClient {
  request<T = unknown>(method: string, params?: unknown): Promise<T>;
}

export interface DaemonClientOptions {
  readonly socketPath?: string;
  readonly timeoutMs?: number;
}

export class DaemonNotRunningError extends Error {
  readonly socketPath: string;

  constructor(socketPath: string, cause: Error) {
    super(`daemon is not running at ${socketPath}`, { cause });
    this.name = "DaemonNotRunningError";
    this.socketPath = socketPath;
  }
}

class DefaultDaemonClient implements DaemonClient {
  private readonly socketPath: string;
  private readonly timeoutMs: number;
  private nextRequestId = 1;

  constructor(options: DaemonClientOptions = {}) {
    this.socketPath = options.socketPath ?? defaultSocketPath();
    this.timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  }

  async request<T>(method: string, params?: unknown): Promise<T> {
    const id = this.nextRequestId;
    this.nextRequestId += 1;

    const payload: JsonRpcRequest = {
      jsonrpc: "2.0",
      id,
      method,
      ...(params === undefined ? {} : { params }),
    };
    const serialized = `${JSON.stringify(payload)}\n`;
    const responseLine = await sendRequestAndReadResponseLine(
      this.socketPath,
      serialized,
      this.timeoutMs,
    );

    let response: JsonRpcResponse<T>;
    try {
      response = JSON.parse(responseLine) as JsonRpcResponse<T>;
    } catch (error) {
      throw new Error("failed to parse daemon json-rpc response", {
        cause: error,
      });
    }

    if (response.error) {
      throw new Error(
        `daemon json-rpc error ${response.error.code}: ${response.error.message}`,
      );
    }

    if (!("result" in response)) {
      throw new Error("daemon json-rpc response missing result field");
    }

    return response.result as T;
  }
}

export function createDaemonClient(
  options: DaemonClientOptions = {},
): DaemonClient {
  return new DefaultDaemonClient(options);
}

function defaultSocketPath(): string {
  if (process.platform === "win32") {
    return WINDOWS_NAMED_PIPE_PATH;
  }

  return join(homedir(), UNIX_SOCKET_RELATIVE_PATH);
}

function isDaemonNotRunningCode(code: string | undefined): boolean {
  return code === "ENOENT" || code === "ECONNREFUSED";
}

async function sendRequestAndReadResponseLine(
  socketPath: string,
  payload: string,
  timeoutMs: number,
): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    const socket = createConnection(socketPath);

    let settled = false;
    let responseBuffer = "";

    const timeoutHandle = setTimeout(() => {
      fail(
        new Error(`timed out waiting for daemon response after ${timeoutMs}ms`),
      );
    }, timeoutMs);

    const cleanup = () => {
      clearTimeout(timeoutHandle);
      socket.removeAllListeners();
    };

    const fail = (error: Error) => {
      if (settled) {
        return;
      }
      settled = true;
      cleanup();
      socket.destroy();
      reject(error);
    };

    const complete = (line: string) => {
      if (settled) {
        return;
      }
      settled = true;
      cleanup();
      socket.end();
      resolve(line);
    };

    socket.on("error", (error: NodeJS.ErrnoException) => {
      if (isDaemonNotRunningCode(error.code)) {
        fail(new DaemonNotRunningError(socketPath, error));
        return;
      }

      fail(error);
    });

    socket.on("connect", () => {
      socket.write(payload, (error) => {
        if (error) {
          fail(error);
        }
      });
    });

    socket.on("data", (chunk: Buffer) => {
      responseBuffer += chunk.toString("utf8");

      const newlineIndex = responseBuffer.indexOf("\n");
      if (newlineIndex === -1) {
        return;
      }

      const line = responseBuffer.slice(0, newlineIndex).trim();
      if (line.length === 0) {
        fail(new Error("daemon returned an empty json-rpc response"));
        return;
      }

      complete(line);
    });

    socket.on("end", () => {
      if (settled) {
        return;
      }

      const trimmed = responseBuffer.trim();
      if (trimmed.length === 0) {
        fail(new Error("daemon returned an empty json-rpc response"));
        return;
      }

      complete(trimmed);
    });
  });
}
