import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { installMockLocalStorage } from "../test/mock-local-storage";
import {
  clearOAuthFlow,
  clearSession,
  loadOAuthFlow,
  loadSession,
  type StoredSession,
  saveOAuthFlow,
  saveSession,
} from "./storage";

describe("auth storage", () => {
  let cleanup: () => void;

  beforeEach(() => {
    cleanup = installMockLocalStorage();
  });

  afterEach(() => {
    cleanup();
  });

  const session: StoredSession = {
    accessToken: "at-1",
    accessExpiresAt: "2099-01-01T00:00:00Z",
    refreshToken: "rt-1",
    refreshExpiresAt: "2099-02-01T00:00:00Z",
    user: { id: "u1", email: "a@b.c", display_name: "Alice" },
  };

  it("saveSession + loadSession round-trips", () => {
    saveSession(session);
    const loaded = loadSession();
    expect(loaded).toEqual(session);
  });

  it("loadSession returns null when fields missing", () => {
    expect(loadSession()).toBeNull();
  });

  it("loadSession returns null for malformed user JSON", () => {
    localStorage.setItem("scriptum:access_token", "at");
    localStorage.setItem("scriptum:access_expires_at", "exp");
    localStorage.setItem("scriptum:refresh_token", "rt");
    localStorage.setItem("scriptum:refresh_expires_at", "rexp");
    localStorage.setItem("scriptum:user", "not-json");
    expect(loadSession()).toBeNull();
  });

  it("loadSession returns null for user missing required fields", () => {
    localStorage.setItem("scriptum:access_token", "at");
    localStorage.setItem("scriptum:access_expires_at", "exp");
    localStorage.setItem("scriptum:refresh_token", "rt");
    localStorage.setItem("scriptum:refresh_expires_at", "rexp");
    localStorage.setItem("scriptum:user", JSON.stringify({ id: "u1" }));
    expect(loadSession()).toBeNull();
  });

  it("clearSession removes all keys", () => {
    saveSession(session);
    clearSession();
    expect(loadSession()).toBeNull();
  });

  it("saveOAuthFlow + loadOAuthFlow round-trips", () => {
    const flow = { state: "s1", codeVerifier: "cv1", flowId: "f1" };
    saveOAuthFlow(flow);
    expect(loadOAuthFlow()).toEqual(flow);
  });

  it("loadOAuthFlow returns null when missing", () => {
    expect(loadOAuthFlow()).toBeNull();
  });

  it("clearOAuthFlow removes flow keys", () => {
    saveOAuthFlow({ state: "s", codeVerifier: "v", flowId: "f" });
    clearOAuthFlow();
    expect(loadOAuthFlow()).toBeNull();
  });
});
