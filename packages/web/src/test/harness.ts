import { isFixtureModeEnabled } from "./setup";

export type SyncState = "synced" | "offline" | "reconnecting" | "error";

export interface CursorPosition {
  line: number;
  ch: number;
}

export interface RemotePeer {
  name: string;
  type: "human" | "agent";
  cursor: CursorPosition;
  section?: string;
}

export interface GitStatus {
  dirty: boolean;
  ahead: number;
  behind: number;
  lastCommit?: string;
}

export interface ScriptumTestState {
  fixtureName: string;
  docContent: string;
  cursor: CursorPosition;
  remotePeers: RemotePeer[];
  syncState: SyncState;
  gitStatus: GitStatus;
  commentThreads: unknown[];
}

export interface ScriptumTestApi {
  loadFixture(name: string): void;
  setDocContent(markdown: string): void;
  setCursor(pos: CursorPosition): void;
  spawnRemotePeer(peer: RemotePeer): void;
  setGitStatus(status: GitStatus): void;
  setSyncState(state: SyncState): void;
  setCommentThreads(threads: unknown[]): void;
  getState(): ScriptumTestState;
  reset(): void;
  subscribe(listener: (state: ScriptumTestState) => void): () => void;
}

export interface InstallScriptumTestApiOptions {
  env?: Record<string, unknown>;
  target?: Window & typeof globalThis;
  initialState?: Partial<ScriptumTestState>;
}

const DEFAULT_STATE: ScriptumTestState = {
  fixtureName: "default",
  docContent: "# Fixture Document",
  cursor: { line: 0, ch: 0 },
  remotePeers: [],
  syncState: "synced",
  gitStatus: { dirty: false, ahead: 0, behind: 0 },
  commentThreads: [],
};

const FIXTURES: Record<string, Partial<ScriptumTestState>> = {
  default: DEFAULT_STATE,
  "overlap-warning": {
    fixtureName: "overlap-warning",
    docContent:
      "# Overlap Warning\n\n## Shared Section\nTwo collaborators are editing this section.",
    cursor: { line: 2, ch: 0 },
    remotePeers: [
      {
        name: "Relay Agent",
        type: "agent",
        cursor: { line: 2, ch: 5 },
        section: "Shared Section",
      },
    ],
    syncState: "reconnecting",
    gitStatus: { dirty: true, ahead: 1, behind: 0 },
  },
};

declare global {
  interface Window {
    __SCRIPTUM_TEST__?: ScriptumTestApi;
  }
}

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function assertCursorPosition(value: CursorPosition): void {
  if (!Number.isInteger(value.line) || value.line < 0) {
    throw new Error("cursor.line must be a non-negative integer");
  }
  if (!Number.isInteger(value.ch) || value.ch < 0) {
    throw new Error("cursor.ch must be a non-negative integer");
  }
}

function withDefaults(initialState?: Partial<ScriptumTestState>): ScriptumTestState {
  const merged: ScriptumTestState = {
    ...DEFAULT_STATE,
    ...initialState,
    cursor: initialState?.cursor ?? DEFAULT_STATE.cursor,
    remotePeers: initialState?.remotePeers ?? DEFAULT_STATE.remotePeers,
    gitStatus: initialState?.gitStatus ?? DEFAULT_STATE.gitStatus,
    commentThreads: initialState?.commentThreads ?? DEFAULT_STATE.commentThreads,
  };
  return clone(merged);
}

export function createScriptumTestApi(
  initialState?: Partial<ScriptumTestState>
): ScriptumTestApi {
  let state = withDefaults(initialState);
  const listeners = new Set<(value: ScriptumTestState) => void>();

  const emit = () => {
    const snapshot = clone(state);
    for (const listener of listeners) {
      listener(snapshot);
    }
  };

  return {
    loadFixture(name: string) {
      const fixture = FIXTURES[name];
      if (!fixture) {
        throw new Error(`unknown fixture: ${name}`);
      }
      state = withDefaults({ ...fixture, fixtureName: name });
      emit();
    },

    setDocContent(markdown: string) {
      state.docContent = markdown;
      emit();
    },

    setCursor(pos: CursorPosition) {
      assertCursorPosition(pos);
      state.cursor = clone(pos);
      emit();
    },

    spawnRemotePeer(peer: RemotePeer) {
      assertCursorPosition(peer.cursor);
      state.remotePeers = [...state.remotePeers, clone(peer)];
      emit();
    },

    setGitStatus(status: GitStatus) {
      state.gitStatus = clone(status);
      emit();
    },

    setSyncState(syncState: SyncState) {
      state.syncState = syncState;
      emit();
    },

    setCommentThreads(threads: unknown[]) {
      state.commentThreads = clone(threads);
      emit();
    },

    getState() {
      return clone(state);
    },

    reset() {
      state = withDefaults();
      emit();
    },

    subscribe(listener: (value: ScriptumTestState) => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

export function installScriptumTestApi(
  options: InstallScriptumTestApiOptions = {}
): ScriptumTestApi | undefined {
  if (!isFixtureModeEnabled(options.env)) {
    return undefined;
  }

  const target =
    options.target ??
    (typeof window === "undefined" ? undefined : window);
  if (!target) {
    return undefined;
  }

  if (target.__SCRIPTUM_TEST__) {
    return target.__SCRIPTUM_TEST__;
  }

  const api = createScriptumTestApi(options.initialState);
  Object.defineProperty(target, "__SCRIPTUM_TEST__", {
    value: api,
    writable: false,
    configurable: true,
  });
  return api;
}
