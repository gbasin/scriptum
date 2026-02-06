import type {
  GitStatus,
  RemotePeer,
  ScriptumTestApi,
  SyncState,
} from "./harness";

export interface SmokeFixture {
  id: string;
  route: string;
  description: string;
  loadFixture?: string;
  docContent?: string;
  syncState?: SyncState;
  gitStatus?: GitStatus;
  remotePeers?: RemotePeer[];
}

const DEFAULT_ROUTE = "/workspace/ws-smoke/document/doc-smoke";

export const SMOKE_FIXTURES: SmokeFixture[] = [
  {
    id: "editor-default",
    route: DEFAULT_ROUTE,
    description: "Default editor shell and sidebar with synced state",
    docContent: "# Smoke: Default\n\nLocal-first markdown editing baseline.",
    syncState: "synced",
  },
  {
    id: "editor-offline",
    route: DEFAULT_ROUTE,
    description: "Offline sync indicator with unsynced content",
    docContent: "# Smoke: Offline\n\nPending local updates while disconnected.",
    syncState: "offline",
    gitStatus: { dirty: true, ahead: 2, behind: 0, lastCommit: "abc123" },
  },
  {
    id: "presence-dual",
    route: DEFAULT_ROUTE,
    description: "Two collaborators visible in presence stack",
    docContent: "# Smoke: Presence\n\nCollaborators are editing this section.",
    syncState: "synced",
    remotePeers: [
      { name: "Alex", type: "human", cursor: { line: 2, ch: 3 } },
      { name: "Scriptum Agent", type: "agent", cursor: { line: 2, ch: 12 } },
    ],
  },
  {
    id: "sync-reconnecting",
    route: DEFAULT_ROUTE,
    description: "Reconnect state with overlap-warning fixture",
    loadFixture: "overlap-warning",
    syncState: "reconnecting",
  },
  {
    id: "sync-error",
    route: DEFAULT_ROUTE,
    description: "Error state for smoke coverage",
    docContent: "# Smoke: Error\n\nManual retry suggested by status UI.",
    syncState: "error",
  },
];

export function applySmokeFixture(
  api: ScriptumTestApi,
  fixture: SmokeFixture,
): void {
  api.reset();
  if (fixture.loadFixture) {
    api.loadFixture(fixture.loadFixture);
  }
  if (fixture.docContent !== undefined) {
    api.setDocContent(fixture.docContent);
  }
  if (fixture.syncState !== undefined) {
    api.setSyncState(fixture.syncState);
  }
  if (fixture.gitStatus !== undefined) {
    api.setGitStatus(fixture.gitStatus);
  }
  if (fixture.remotePeers) {
    for (const peer of fixture.remotePeers) {
      api.spawnRemotePeer(peer);
    }
  }
}
