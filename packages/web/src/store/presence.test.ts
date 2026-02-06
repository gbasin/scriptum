import { describe, expect, it } from "vitest";
import * as Y from "yjs";
import {
  bindPresenceStoreToYjs,
  createPresenceStore,
  type PeerPresence,
} from "./presence";

const ALICE: PeerPresence = {
  name: "alice",
  type: "human",
  activeDocumentPath: "docs/readme.md",
  cursor: { sectionId: "sec-1", line: 5, column: 12 },
  lastSeenAt: "2026-01-15T10:00:00Z",
  color: "#e74c3c",
};

const BOB: PeerPresence = {
  name: "bob",
  type: "agent",
  activeDocumentPath: "docs/api.md",
  cursor: null,
  lastSeenAt: "2026-01-15T10:01:00Z",
  color: "#3498db",
};

describe("presence store", () => {
  it("tracks peers and computes remote peers", () => {
    const store = createPresenceStore({ localPeerName: "alice" });

    store.getState().setPeers([ALICE, BOB]);
    expect(store.getState().peers).toHaveLength(2);
    expect(store.getState().onlineCount).toBe(2);
    expect(store.getState().remotePeers).toHaveLength(1);
    expect(store.getState().remotePeers[0].name).toBe("bob");
  });

  it("upserts a peer", () => {
    const store = createPresenceStore();

    store.getState().upsertPeer(ALICE);
    expect(store.getState().peers).toHaveLength(1);

    const updated: PeerPresence = {
      ...ALICE,
      cursor: { sectionId: "sec-2", line: 10, column: 0 },
    };
    store.getState().upsertPeer(updated);
    expect(store.getState().peers).toHaveLength(1);
    expect(store.getState().peers[0].cursor?.line).toBe(10);
  });

  it("removes a peer", () => {
    const store = createPresenceStore();
    store.getState().setPeers([ALICE, BOB]);
    expect(store.getState().peers).toHaveLength(2);

    store.getState().removePeer("bob");
    expect(store.getState().peers).toHaveLength(1);
    expect(store.getState().peers[0].name).toBe("alice");
  });

  it("sets local peer name and filters remote peers", () => {
    const store = createPresenceStore();
    store.getState().setPeers([ALICE, BOB]);
    expect(store.getState().remotePeers).toHaveLength(2);

    store.getState().setLocalPeerName("alice");
    expect(store.getState().remotePeers).toHaveLength(1);
    expect(store.getState().remotePeers[0].name).toBe("bob");
  });

  it("resets to empty state", () => {
    const store = createPresenceStore({ localPeerName: "alice" });
    store.getState().setPeers([ALICE, BOB]);
    store.getState().reset();

    expect(store.getState().peers).toHaveLength(0);
    expect(store.getState().localPeerName).toBeNull();
    expect(store.getState().onlineCount).toBe(0);
  });

  it("reacts to Yjs presence map updates", () => {
    const doc = new Y.Doc();
    const store = createPresenceStore();
    const stopBinding = bindPresenceStoreToYjs(doc, { store });
    const presenceMap = doc.getMap<unknown>("presence");

    doc.transact(() => {
      presenceMap.set("alice", ALICE);
      presenceMap.set("bob", BOB);
    });

    expect(store.getState().peers).toHaveLength(2);
    expect(
      store.getState().peers.map((p) => p.name).sort()
    ).toEqual(["alice", "bob"]);

    doc.transact(() => {
      presenceMap.delete("bob");
    });

    expect(store.getState().peers).toHaveLength(1);
    expect(store.getState().peers[0].name).toBe("alice");

    stopBinding();

    doc.transact(() => {
      presenceMap.delete("alice");
    });

    // After unbinding, store should still have alice
    expect(store.getState().peers).toHaveLength(1);
  });

  it("ignores invalid presence entries from Yjs", () => {
    const doc = new Y.Doc();
    const store = createPresenceStore();
    const stopBinding = bindPresenceStoreToYjs(doc, { store });
    const presenceMap = doc.getMap<unknown>("presence");

    doc.transact(() => {
      presenceMap.set("valid", ALICE);
      presenceMap.set("invalid", { name: 123 }); // invalid: name must be string
      presenceMap.set("incomplete", { name: "x" }); // missing required fields
    });

    expect(store.getState().peers).toHaveLength(1);
    expect(store.getState().peers[0].name).toBe("alice");

    stopBinding();
  });
});
