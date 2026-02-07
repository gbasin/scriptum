// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, } from "vitest";
import { createSyncStore, type SyncStore } from "../store/sync";
import {
  type UseSyncStatusOptions,
  type UseSyncStatusResult,
  useSyncStatus,
} from "./useSyncStatus";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

type ProviderStatus = "connected" | "disconnected";

class FakeSocketProvider {
  private statusHandler: ((event: { status: ProviderStatus }) => void) | null =
    null;

  on(
    _event: "status",
    handler: (event: { status: ProviderStatus }) => void,
  ): void {
    this.statusHandler = handler;
  }

  emit(status: ProviderStatus): void {
    this.statusHandler?.({ status });
  }
}

class FakeCollaborationProvider {
  readonly provider = new FakeSocketProvider();
}

function renderUseSyncStatus(initialOptions: UseSyncStatusOptions) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  let latest: UseSyncStatusResult | null = null;
  let currentOptions = initialOptions;

  function Probe(props: { options: UseSyncStatusOptions }) {
    latest = useSyncStatus(props.options);
    return null;
  }

  const render = () => {
    act(() => {
      root.render(<Probe options={currentOptions} />);
    });
  };

  render();

  return {
    latest: () => {
      if (!latest) {
        throw new Error("hook did not produce a value");
      }
      return latest;
    },
    rerender: (nextOptions: UseSyncStatusOptions) => {
      currentOptions = nextOptions;
      render();
    },
    unmount: () => {
      act(() => {
        root.unmount();
      });
    },
  };
}

function createStore(): SyncStore {
  return createSyncStore();
}

describe("useSyncStatus", () => {
  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  });

  afterEach(() => {
    document.body.innerHTML = "";
    globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
  });

  it("marks sync as synced on provider connected and records lastSyncedAt", () => {
    const store = createStore();
    const provider = new FakeCollaborationProvider();
    const harness = renderUseSyncStatus({
      provider: provider as unknown as UseSyncStatusOptions["provider"],
      store,
    });

    act(() => {
      provider.provider.emit("connected");
    });

    const state = harness.latest();
    expect(state.status).toBe("synced");
    expect(state.lastSyncedAt).not.toBeNull();
    expect(store.getState().status).toBe("online");

    harness.unmount();
  });

  it("derives syncing when online with pending changes", () => {
    const store = createStore();
    const provider = new FakeCollaborationProvider();
    const harness = renderUseSyncStatus({
      pendingChangesCount: 4,
      provider: provider as unknown as UseSyncStatusOptions["provider"],
      store,
    });

    act(() => {
      provider.provider.emit("connected");
    });

    expect(harness.latest().status).toBe("syncing");
    expect(harness.latest().pendingChangesCount).toBe(4);

    harness.unmount();
  });

  it("marks sync as reconnecting after disconnect", () => {
    const store = createStore();
    const provider = new FakeCollaborationProvider();
    const harness = renderUseSyncStatus({
      provider: provider as unknown as UseSyncStatusOptions["provider"],
      store,
    });

    act(() => {
      provider.provider.emit("connected");
      provider.provider.emit("disconnected");
    });

    expect(harness.latest().status).toBe("reconnecting");
    expect(store.getState().status).toBe("reconnecting");

    harness.unmount();
  });

  it("returns error status when store has an error", () => {
    const store = createStore();
    store.getState().setOffline("connection failed");

    const harness = renderUseSyncStatus({
      provider: null,
      store,
    });

    expect(harness.latest().status).toBe("error");
    expect(harness.latest().error).toBe("connection failed");

    harness.unmount();
  });

  it("exposes reconnect progress for backlog sync display", () => {
    const store = createStore();
    const provider = new FakeCollaborationProvider();
    const harness = renderUseSyncStatus({
      provider: provider as unknown as UseSyncStatusOptions["provider"],
      reconnectProgress: { syncedUpdates: 847, totalUpdates: 1203 },
      store,
    });

    act(() => {
      provider.provider.emit("connected");
    });

    const state = harness.latest();
    expect(state.status).toBe("syncing");
    expect(state.reconnectProgress).toEqual({
      syncedUpdates: 847,
      totalUpdates: 1203,
    });

    harness.unmount();
  });
});
