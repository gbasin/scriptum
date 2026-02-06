import { describe, expect, it } from "vitest";
import { createSyncStore } from "./sync";

describe("sync store", () => {
  it("starts offline by default", () => {
    const store = createSyncStore();
    expect(store.getState().status).toBe("offline");
    expect(store.getState().lastSyncedAt).toBeNull();
    expect(store.getState().pendingChanges).toBe(0);
    expect(store.getState().error).toBeNull();
    expect(store.getState().reconnectAttempt).toBe(0);
  });

  it("transitions to online", () => {
    const store = createSyncStore();
    store.getState().setOnline();
    expect(store.getState().status).toBe("online");
    expect(store.getState().error).toBeNull();
    expect(store.getState().reconnectAttempt).toBe(0);
  });

  it("transitions to offline with error", () => {
    const store = createSyncStore({ status: "online" });
    store.getState().setOffline("connection lost");
    expect(store.getState().status).toBe("offline");
    expect(store.getState().error).toBe("connection lost");
  });

  it("transitions to offline without error", () => {
    const store = createSyncStore({ status: "online" });
    store.getState().setOffline();
    expect(store.getState().status).toBe("offline");
    expect(store.getState().error).toBeNull();
  });

  it("increments reconnect attempt", () => {
    const store = createSyncStore();
    store.getState().setReconnecting();
    expect(store.getState().status).toBe("reconnecting");
    expect(store.getState().reconnectAttempt).toBe(1);

    store.getState().setReconnecting();
    expect(store.getState().reconnectAttempt).toBe(2);

    store.getState().setReconnecting();
    expect(store.getState().reconnectAttempt).toBe(3);
  });

  it("clears reconnect attempt on online", () => {
    const store = createSyncStore();
    store.getState().setReconnecting();
    store.getState().setReconnecting();
    expect(store.getState().reconnectAttempt).toBe(2);

    store.getState().setOnline();
    expect(store.getState().reconnectAttempt).toBe(0);
  });

  it("tracks last synced timestamp", () => {
    const store = createSyncStore();
    store.getState().setOnline();
    store.getState().setLastSyncedAt("2026-01-15T10:00:00Z");
    expect(store.getState().lastSyncedAt).toBe("2026-01-15T10:00:00Z");
  });

  it("tracks pending changes", () => {
    const store = createSyncStore();
    store.getState().setPendingChanges(5);
    expect(store.getState().pendingChanges).toBe(5);

    store.getState().setPendingChanges(0);
    expect(store.getState().pendingChanges).toBe(0);
  });

  it("resets to initial state", () => {
    const store = createSyncStore();
    store.getState().setOnline();
    store.getState().setLastSyncedAt("2026-01-15T10:00:00Z");
    store.getState().setPendingChanges(3);

    store.getState().reset();
    expect(store.getState().status).toBe("offline");
    expect(store.getState().lastSyncedAt).toBeNull();
    expect(store.getState().pendingChanges).toBe(0);
    expect(store.getState().error).toBeNull();
    expect(store.getState().reconnectAttempt).toBe(0);
  });

  it("supports full lifecycle: offline -> reconnecting -> online -> offline", () => {
    const store = createSyncStore();

    // Start offline
    expect(store.getState().status).toBe("offline");

    // Begin reconnecting
    store.getState().setReconnecting();
    expect(store.getState().status).toBe("reconnecting");
    expect(store.getState().reconnectAttempt).toBe(1);

    // Failed attempt
    store.getState().setReconnecting();
    expect(store.getState().reconnectAttempt).toBe(2);

    // Success
    store.getState().setOnline();
    expect(store.getState().status).toBe("online");
    expect(store.getState().reconnectAttempt).toBe(0);
    store.getState().setLastSyncedAt("2026-01-15T10:00:00Z");
    store.getState().setPendingChanges(2);

    // Disconnect
    store.getState().setOffline("server unreachable");
    expect(store.getState().status).toBe("offline");
    expect(store.getState().error).toBe("server unreachable");
    expect(store.getState().lastSyncedAt).toBe("2026-01-15T10:00:00Z");
    expect(store.getState().pendingChanges).toBe(2);
  });

  it("accepts initial state overrides", () => {
    const store = createSyncStore({
      status: "online",
      lastSyncedAt: "2026-01-01T00:00:00Z",
      pendingChanges: 7,
    });
    expect(store.getState().status).toBe("online");
    expect(store.getState().lastSyncedAt).toBe("2026-01-01T00:00:00Z");
    expect(store.getState().pendingChanges).toBe(7);
  });
});
