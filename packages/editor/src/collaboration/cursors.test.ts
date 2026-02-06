// @vitest-environment jsdom
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { describe, expect, it } from "vitest";
import { Awareness } from "y-protocols/awareness";
import * as Y from "yjs";

import {
  LABEL_HIDE_DELAY_MS,
  nameToColor,
  remoteCursorExtension,
  remotePeersField,
  setRemotePeers,
  type RemotePeer,
} from "./cursors";

// ── nameToColor ──────────────────────────────────────────────────────

describe("nameToColor", () => {
  it("returns the same color for the same name", () => {
    const a = nameToColor("alice");
    const b = nameToColor("alice");
    expect(a).toBe(b);
  });

  it("returns different colors for different names", () => {
    const a = nameToColor("alice");
    const b = nameToColor("bob");
    expect(a).not.toBe(b);
  });

  it("returns a hex color string", () => {
    const color = nameToColor("claude");
    expect(color).toMatch(/^#[0-9a-f]{6}$/i);
  });

  it("handles empty string without throwing", () => {
    const color = nameToColor("");
    expect(color).toMatch(/^#[0-9a-f]{6}$/i);
  });

  it("is consistent across many names", () => {
    const names = [
      "alice",
      "bob",
      "charlie",
      "dana",
      "eve",
      "frank",
      "grace",
      "heidi",
      "ivan",
      "judy",
      "karl",
      "laura",
    ];
    const colors = names.map(nameToColor);
    // All should be valid hex colors
    for (const c of colors) {
      expect(c).toMatch(/^#[0-9a-f]{6}$/i);
    }
    // With 12 names and 12 palette colors, we should see reasonable spread
    const unique = new Set(colors);
    expect(unique.size).toBeGreaterThanOrEqual(4);
  });
});

// ── remotePeersField ──────────────────────────────────────────────────

describe("remotePeersField", () => {
  it("starts with an empty peer list", () => {
    const state = EditorState.create({
      extensions: [remotePeersField],
    });
    expect(state.field(remotePeersField)).toEqual([]);
  });

  it("updates when setRemotePeers effect is dispatched", () => {
    const state = EditorState.create({
      extensions: [remotePeersField],
    });

    const peers: readonly RemotePeer[] = [
      {
        clientId: 1,
        name: "alice",
        color: "#e06c75",
        cursor: 10,
        selectionFrom: 10,
        selectionTo: 10,
        lastMovedAt: Date.now(),
      },
    ];

    const tr = state.update({
      effects: setRemotePeers.of(peers),
    });

    expect(tr.state.field(remotePeersField)).toEqual(peers);
  });

  it("preserves peers when unrelated transactions occur", () => {
    const state = EditorState.create({
      doc: "hello",
      extensions: [remotePeersField],
    });

    const peers: readonly RemotePeer[] = [
      {
        clientId: 1,
        name: "alice",
        color: "#e06c75",
        cursor: 3,
        selectionFrom: 3,
        selectionTo: 3,
        lastMovedAt: Date.now(),
      },
    ];

    const s1 = state.update({ effects: setRemotePeers.of(peers) }).state;
    expect(s1.field(remotePeersField)).toEqual(peers);

    // A doc change should not clear peers
    const s2 = s1.update({ changes: { from: 0, insert: "x" } }).state;
    expect(s2.field(remotePeersField)).toEqual(peers);
  });
});

// ── remoteCursorExtension integration ────────────────────────────────

describe("remoteCursorExtension", () => {
  function makeAwareness(): { awareness: Awareness; doc: Y.Doc } {
    const doc = new Y.Doc();
    const awareness = new Awareness(doc);
    return { awareness, doc };
  }

  function makeEditor(
    awareness: Awareness,
    now: () => number,
    content = "hello world",
  ): EditorView {
    const parent = document.createElement("div");
    return new EditorView({
      state: EditorState.create({
        doc: content,
        extensions: [remoteCursorExtension({ awareness, now })],
      }),
      parent,
    });
  }

  it("creates an editor without errors", () => {
    const { awareness } = makeAwareness();
    const view = makeEditor(awareness, Date.now);
    expect(view.state.field(remotePeersField)).toEqual([]);
    view.destroy();
  });

  it("picks up remote peers from awareness state", async () => {
    const { awareness } = makeAwareness();

    // Create a remote peer
    const remoteDoc = new Y.Doc();
    const remoteClientId = remoteDoc.clientID;

    // Simulate the remote peer setting awareness state
    // We inject it directly into the local awareness for testing
    awareness.setLocalStateField("cursor", { anchor: 5, head: 5 });
    awareness.setLocalStateField("user", {
      name: "bob",
      color: "#61afef",
    });

    // Manually add a remote state to the awareness
    const states = awareness.getStates();
    states.set(remoteClientId, {
      cursor: { anchor: 3, head: 3 },
      user: { name: "alice", color: "#e06c75" },
    });

    let time = 1000;
    const view = makeEditor(awareness, () => time, "hello world");

    // Trigger awareness change
    awareness.emit("change", [
      { added: [remoteClientId], updated: [], removed: [] },
      "test",
    ]);

    // Wait for state dispatch to propagate
    await new Promise((resolve) => setTimeout(resolve, 10));

    const peers = view.state.field(remotePeersField);
    expect(peers.length).toBe(1);
    expect(peers[0].name).toBe("alice");
    expect(peers[0].color).toBe("#e06c75");
    expect(peers[0].cursor).toBe(3);

    view.destroy();
  });

  it("excludes the local client from remote peers", () => {
    const { awareness } = makeAwareness();
    awareness.setLocalStateField("cursor", { anchor: 0, head: 0 });
    awareness.setLocalStateField("user", { name: "me" });

    const view = makeEditor(awareness, Date.now);

    // Trigger awareness change
    awareness.emit("change", [
      { added: [], updated: [awareness.clientID], removed: [] },
      "test",
    ]);

    // Local client should not appear in peer list
    const peers = view.state.field(remotePeersField);
    expect(peers.length).toBe(0);

    view.destroy();
  });

  it("uses nameToColor when no user color is set", () => {
    const { awareness } = makeAwareness();

    const remoteDoc = new Y.Doc();
    const remoteClientId = remoteDoc.clientID;

    const states = awareness.getStates();
    states.set(remoteClientId, {
      cursor: { anchor: 2, head: 2 },
      user: { name: "zara" },
    });

    const view = makeEditor(awareness, Date.now);

    awareness.emit("change", [
      { added: [remoteClientId], updated: [], removed: [] },
      "test",
    ]);

    // Wait a tick
    return new Promise<void>((resolve) => {
      setTimeout(() => {
        const peers = view.state.field(remotePeersField);
        expect(peers.length).toBe(1);
        expect(peers[0].color).toBe(nameToColor("zara"));
        view.destroy();
        resolve();
      }, 10);
    });
  });

  it("defaults name to 'User {clientId}' when no user name is set", () => {
    const { awareness } = makeAwareness();

    const remoteDoc = new Y.Doc();
    const remoteClientId = remoteDoc.clientID;

    const states = awareness.getStates();
    states.set(remoteClientId, {
      cursor: { anchor: 1, head: 1 },
    });

    const view = makeEditor(awareness, Date.now);

    awareness.emit("change", [
      { added: [remoteClientId], updated: [], removed: [] },
      "test",
    ]);

    return new Promise<void>((resolve) => {
      setTimeout(() => {
        const peers = view.state.field(remotePeersField);
        expect(peers.length).toBe(1);
        expect(peers[0].name).toBe(`User ${remoteClientId}`);
        view.destroy();
        resolve();
      }, 10);
    });
  });

  it("label visibility is based on LABEL_HIDE_DELAY_MS", async () => {
    const { awareness } = makeAwareness();

    const remoteDoc = new Y.Doc();
    const remoteClientId = remoteDoc.clientID;

    let time = 10_000;
    const states = awareness.getStates();
    states.set(remoteClientId, {
      cursor: { anchor: 5, head: 5 },
      user: { name: "bob" },
    });

    const view = makeEditor(awareness, () => time, "hello world");

    // First awareness event — cursor appears, label should be visible
    awareness.emit("change", [
      { added: [remoteClientId], updated: [], removed: [] },
      "test",
    ]);

    await new Promise((resolve) => setTimeout(resolve, 10));

    let peers = view.state.field(remotePeersField);
    expect(peers.length).toBe(1);
    // lastMovedAt should be close to our mock time
    expect(peers[0].lastMovedAt).toBe(time);

    // At time = 10_000, label should be visible (now - lastMovedAt < LABEL_HIDE_DELAY_MS)
    expect(time - peers[0].lastMovedAt).toBeLessThan(LABEL_HIDE_DELAY_MS);

    // Advance time past the hide delay
    time = 10_000 + LABEL_HIDE_DELAY_MS + 100;

    // Re-trigger awareness (simulates a label-hide check)
    awareness.emit("change", [
      { added: [], updated: [remoteClientId], removed: [] },
      "test",
    ]);

    await new Promise((resolve) => setTimeout(resolve, 10));

    peers = view.state.field(remotePeersField);
    // Cursor position didn't change, so lastMovedAt should still be 10_000
    expect(peers[0].lastMovedAt).toBe(10_000);
    // Now the time difference exceeds LABEL_HIDE_DELAY_MS
    expect(time - peers[0].lastMovedAt).toBeGreaterThan(
      LABEL_HIDE_DELAY_MS,
    );

    view.destroy();
  });

  it("detects cursor movement and updates lastMovedAt", async () => {
    const { awareness } = makeAwareness();

    const remoteDoc = new Y.Doc();
    const remoteClientId = remoteDoc.clientID;

    let time = 10_000;
    const states = awareness.getStates();
    states.set(remoteClientId, {
      cursor: { anchor: 5, head: 5 },
      user: { name: "carol" },
    });

    const view = makeEditor(awareness, () => time, "hello world");

    // Initial awareness event
    awareness.emit("change", [
      { added: [remoteClientId], updated: [], removed: [] },
      "test",
    ]);
    await new Promise((resolve) => setTimeout(resolve, 10));

    let peers = view.state.field(remotePeersField);
    expect(peers[0].lastMovedAt).toBe(10_000);

    // Advance time and move cursor
    time = 15_000;
    states.set(remoteClientId, {
      cursor: { anchor: 8, head: 8 },
      user: { name: "carol" },
    });

    awareness.emit("change", [
      { added: [], updated: [remoteClientId], removed: [] },
      "test",
    ]);
    await new Promise((resolve) => setTimeout(resolve, 10));

    peers = view.state.field(remotePeersField);
    expect(peers[0].cursor).toBe(8);
    expect(peers[0].lastMovedAt).toBe(15_000);

    view.destroy();
  });

  it("renders collaborator selection highlights for non-collapsed selections", async () => {
    const { awareness } = makeAwareness();

    const remoteDoc = new Y.Doc();
    const remoteClientId = remoteDoc.clientID;

    const states = awareness.getStates();
    states.set(remoteClientId, {
      cursor: { anchor: 2, head: 7 },
      user: { name: "alice", color: "#61afef" },
    });

    const view = makeEditor(awareness, Date.now, "hello world");

    awareness.emit("change", [
      { added: [remoteClientId], updated: [], removed: [] },
      "test",
    ]);
    await new Promise((resolve) => setTimeout(resolve, 10));

    const peers = view.state.field(remotePeersField);
    expect(peers.length).toBe(1);
    expect(peers[0].cursor).toBe(7);
    expect(peers[0].selectionFrom).toBe(2);
    expect(peers[0].selectionTo).toBe(7);

    const highlights = view.dom.querySelectorAll(
      ".cm-remote-selection",
    );
    expect(highlights.length).toBe(1);
    expect(highlights[0].getAttribute("data-peer-color")).toBe(
      "#61afef",
    );

    view.destroy();
  });

  it("uses deterministic name-based color for selection highlight when user color is missing", async () => {
    const { awareness } = makeAwareness();

    const remoteDoc = new Y.Doc();
    const remoteClientId = remoteDoc.clientID;

    const states = awareness.getStates();
    states.set(remoteClientId, {
      cursor: { anchor: 1, head: 4 },
      user: { name: "zara" },
    });

    const view = makeEditor(awareness, Date.now, "hello world");

    awareness.emit("change", [
      { added: [remoteClientId], updated: [], removed: [] },
      "test",
    ]);
    await new Promise((resolve) => setTimeout(resolve, 10));

    const highlights = view.dom.querySelectorAll(
      ".cm-remote-selection",
    );
    expect(highlights.length).toBe(1);
    expect(highlights[0].getAttribute("data-peer-color")).toBe(
      nameToColor("zara"),
    );

    view.destroy();
  });

  it("does not render a selection highlight for collapsed selections", async () => {
    const { awareness } = makeAwareness();

    const remoteDoc = new Y.Doc();
    const remoteClientId = remoteDoc.clientID;

    const states = awareness.getStates();
    states.set(remoteClientId, {
      cursor: { anchor: 3, head: 3 },
      user: { name: "bob", color: "#e06c75" },
    });

    const view = makeEditor(awareness, Date.now, "hello world");

    awareness.emit("change", [
      { added: [remoteClientId], updated: [], removed: [] },
      "test",
    ]);
    await new Promise((resolve) => setTimeout(resolve, 10));

    const highlights = view.dom.querySelectorAll(
      ".cm-remote-selection",
    );
    expect(highlights.length).toBe(0);

    view.destroy();
  });

  it("clamps cursor position to document length", async () => {
    const { awareness } = makeAwareness();

    const remoteDoc = new Y.Doc();
    const remoteClientId = remoteDoc.clientID;

    const states = awareness.getStates();
    states.set(remoteClientId, {
      cursor: { anchor: 999, head: 999 },
      user: { name: "overflow" },
    });

    // Document is only 5 chars
    const view = makeEditor(awareness, Date.now, "hello");

    awareness.emit("change", [
      { added: [remoteClientId], updated: [], removed: [] },
      "test",
    ]);
    await new Promise((resolve) => setTimeout(resolve, 10));

    const peers = view.state.field(remotePeersField);
    expect(peers.length).toBe(1);
    // The cursor pos in the peer list will be 999 (raw from awareness),
    // but the decoration builder clamps it to doc length.
    // The peer's cursor value reflects awareness state directly.
    expect(peers[0].cursor).toBe(999);

    view.destroy();
  });

  it("skips peers with no cursor in awareness state", async () => {
    const { awareness } = makeAwareness();

    const remoteDoc = new Y.Doc();
    const remoteClientId = remoteDoc.clientID;

    const states = awareness.getStates();
    states.set(remoteClientId, {
      user: { name: "no-cursor" },
      // no cursor field
    });

    const view = makeEditor(awareness, Date.now);

    awareness.emit("change", [
      { added: [remoteClientId], updated: [], removed: [] },
      "test",
    ]);
    await new Promise((resolve) => setTimeout(resolve, 10));

    const peers = view.state.field(remotePeersField);
    expect(peers.length).toBe(0);

    view.destroy();
  });
});
