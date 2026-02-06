export interface DaemonClient {
  request(method: string, params?: unknown): Promise<unknown>;
}

export function createDaemonClient(): DaemonClient {
  return {
    async request() {
      throw new Error("Daemon client not implemented");
    },
  };
}
