// @vitest-environment jsdom

import { nameToColor } from "@scriptum/editor";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePresence, type PresenceCursor, type UsePresenceResult } from "./usePresence";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

type AwarenessStatusHandler = () => void;

class FakeAwareness {
  readonly clientID: number;
  readonly on = vi.fn((event: "change", handler: AwarenessStatusHandler) => {
    if (event !== "change") {
      return;
    }
    this.handlers.add(handler);
  });
  readonly off = vi.fn((event: "change", handler: AwarenessStatusHandler) => {
    if (event !== "change") {
      return;
    }
    this.handlers.delete(handler);
  });
  readonly setLocalStateField = vi.fn((field: string, value: unknown) => {
    const current = (this.states.get(this.clientID) ?? {}) as Record<string, unknown>;
    this.states.set(this.clientID, { ...current, [field]: value });
    this.emitChange();
  });

  private readonly handlers = new Set<AwarenessStatusHandler>();
  private readonly states = new Map<number, unknown>();

  constructor(clientID: number) {
    this.clientID = clientID;
  }

  getStates(): Map<number, unknown> {
    return this.states;
  }

  getLocalState(): unknown {
    return this.states.get(this.clientID) ?? null;
  }

  setPeerState(clientId: number, state: unknown): void {
    this.states.set(clientId, state);
    this.emitChange();
  }

  removePeerState(clientId: number): void {
    this.states.delete(clientId);
    this.emitChange();
  }

  private emitChange(): void {
    for (const handler of this.handlers) {
      handler();
    }
  }
}

function renderUsePresence(awareness: FakeAwareness | null) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  let latest: UsePresenceResult | null = null;

  function Probe(props: { awareness: FakeAwareness | null }) {
    latest = usePresence({
      awareness: props.awareness as never,
    });
    return null;
  }

  act(() => {
    root.render(<Probe awareness={awareness} />);
  });

  return {
    latest: () => {
      if (!latest) {
        throw new Error("hook result unavailable");
      }
      return latest;
    },
    unmount: () => {
      act(() => {
        root.unmount();
      });
    },
  };
}

describe("usePresence", () => {
  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  });

  afterEach(() => {
    document.body.innerHTML = "";
    globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
    vi.clearAllMocks();
  });

  it("subscribes to awareness and returns connected peers + local peer", () => {
    const awareness = new FakeAwareness(1);
    awareness.setPeerState(1, {
      cursor: { anchor: 2, head: 4 },
      user: { name: "Local User", type: "human" },
    });
    awareness.setPeerState(9, {
      cursor: { anchor: 8, head: 8 },
      user: { name: "Remote Agent", type: "agent" },
    });
    awareness.setPeerState(5, {
      cursor: { anchor: 6, head: 7 },
      user: { name: "Remote Human" },
    });

    const harness = renderUsePresence(awareness);
    const state = harness.latest();

    expect(awareness.on).toHaveBeenCalledTimes(1);
    expect(state.localPeer).not.toBeNull();
    expect(state.localPeer?.clientId).toBe(1);
    expect(state.localPeer?.name).toBe("Local User");

    expect(state.connectedPeers).toHaveLength(2);
    expect(state.connectedPeers.map((peer) => peer.clientId)).toEqual([5, 9]);
    expect(state.connectedPeers[0]?.type).toBe("human");
    expect(state.connectedPeers[0]?.color).toBe(nameToColor("Remote Human"));
    expect(state.connectedPeers[1]?.type).toBe("agent");
    expect(state.connectedPeers[1]?.color).toBe(nameToColor("Remote Agent"));

    harness.unmount();
  });

  it("setLocalState writes cursor + viewport into awareness", () => {
    const awareness = new FakeAwareness(3);
    awareness.setPeerState(3, {
      user: { name: "Me", type: "agent" },
    });
    const harness = renderUsePresence(awareness);

    const cursor: PresenceCursor = {
      anchor: 11,
      column: 4,
      head: 14,
      line: 7,
      sectionId: "h2:auth",
    };
    const viewport = { fromLine: 5, toLine: 20 };

    act(() => {
      harness.latest().setLocalState(cursor, viewport);
    });

    expect(awareness.setLocalStateField).toHaveBeenCalledWith("user", {
      color: nameToColor("Me"),
      name: "Me",
      type: "agent",
    });
    expect(awareness.setLocalStateField).toHaveBeenCalledWith("cursor", cursor);
    expect(awareness.setLocalStateField).toHaveBeenCalledWith("viewport", viewport);
    expect(harness.latest().localPeer?.cursor).toEqual(cursor);

    harness.unmount();
  });

  it("unsubscribes from awareness on unmount", () => {
    const awareness = new FakeAwareness(11);
    awareness.setPeerState(11, { user: { name: "Local" } });

    const harness = renderUsePresence(awareness);
    expect(awareness.off).toHaveBeenCalledTimes(0);

    harness.unmount();
    expect(awareness.off).toHaveBeenCalledTimes(1);
    expect(awareness.off.mock.calls[0]?.[0]).toBe("change");
    expect(typeof awareness.off.mock.calls[0]?.[1]).toBe("function");
  });

  it("returns empty state when awareness is null", () => {
    const harness = renderUsePresence(null);

    expect(harness.latest().localPeer).toBeNull();
    expect(harness.latest().connectedPeers).toEqual([]);

    act(() => {
      harness.latest().setLocalState({ anchor: 1, head: 1 });
    });

    harness.unmount();
  });
});
