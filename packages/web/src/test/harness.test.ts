import { describe, expect, it, vi } from "vitest";
import {
  createScriptumTestApi,
  installScriptumTestApi,
  type ScriptumTestState,
} from "./harness";

describe("createScriptumTestApi", () => {
  it("updates and exposes deterministic fixture state", () => {
    const api = createScriptumTestApi();

    api.setDocContent("# Updated");
    api.setCursor({ line: 3, ch: 9 });
    api.spawnRemotePeer({
      name: "Assistant",
      type: "agent",
      cursor: { line: 3, ch: 10 },
      section: "Summary",
    });
    api.setSyncState("offline");
    api.setGitStatus({ dirty: true, ahead: 2, behind: 1, lastCommit: "abc123" });

    const state = api.getState();
    expect(state.docContent).toBe("# Updated");
    expect(state.cursor).toEqual({ line: 3, ch: 9 });
    expect(state.remotePeers).toHaveLength(1);
    expect(state.remotePeers[0].name).toBe("Assistant");
    expect(state.syncState).toBe("offline");
    expect(state.gitStatus).toEqual({
      dirty: true,
      ahead: 2,
      behind: 1,
      lastCommit: "abc123",
    });
  });

  it("loads named fixtures and notifies subscribers", () => {
    const api = createScriptumTestApi();
    const listener = vi.fn<(state: ScriptumTestState) => void>();
    const unsubscribe = api.subscribe(listener);

    api.loadFixture("overlap-warning");
    unsubscribe();

    expect(listener).toHaveBeenCalledTimes(1);
    expect(listener.mock.calls[0][0].fixtureName).toBe("overlap-warning");
    expect(api.getState().remotePeers).toHaveLength(1);
  });

  it("rejects invalid cursor positions", () => {
    const api = createScriptumTestApi();

    expect(() => api.setCursor({ line: -1, ch: 0 })).toThrow(
      /cursor\.line must be a non-negative integer/
    );
  });
});

describe("installScriptumTestApi", () => {
  it("installs API only when fixture mode is enabled", () => {
    const targetEnabled = {} as Window & typeof globalThis;
    const enabledApi = installScriptumTestApi({
      env: { VITE_SCRIPTUM_FIXTURE_MODE: "1" },
      target: targetEnabled,
    });
    expect(enabledApi).toBeDefined();
    expect(targetEnabled.__SCRIPTUM_TEST__).toBe(enabledApi);

    const targetDisabled = {} as Window & typeof globalThis;
    const disabledApi = installScriptumTestApi({
      env: { VITE_SCRIPTUM_FIXTURE_MODE: "0" },
      target: targetDisabled,
    });
    expect(disabledApi).toBeUndefined();
    expect(targetDisabled.__SCRIPTUM_TEST__).toBeUndefined();
  });

  it("returns existing installation when called repeatedly", () => {
    const target = {} as Window & typeof globalThis;
    const first = installScriptumTestApi({
      env: { VITE_SCRIPTUM_FIXTURE_MODE: "1" },
      target,
    });
    const second = installScriptumTestApi({
      env: { VITE_SCRIPTUM_FIXTURE_MODE: "1" },
      target,
    });

    expect(first).toBeDefined();
    expect(second).toBe(first);
  });
});
