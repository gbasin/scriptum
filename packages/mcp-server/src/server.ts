export interface ScriptumMcpServer {
  start(): Promise<void>;
}

class StubScriptumMcpServer implements ScriptumMcpServer {
  async start(): Promise<void> {
    // Placeholder start method for scaffold stage.
  }
}

export function createServer(): ScriptumMcpServer {
  return new StubScriptumMcpServer();
}
