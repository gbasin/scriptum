import { describe, expect, it } from "vitest";
import { Awareness } from "y-protocols/awareness";
import * as Y from "yjs";

import { createCollaborationProvider } from "./provider";

class FakeProvider {
  readonly awareness: Awareness;

  connectCalls = 0;
  disconnectCalls = 0;
  destroyCalls = 0;

  private statusHandler: ((event: { status: "connected" | "disconnected" }) => void) | null =
    null;

  constructor(doc: Y.Doc) {
    this.awareness = new Awareness(doc);
  }

  connect(): void {
    this.connectCalls += 1;
    this.statusHandler?.({ status: "connected" });
  }

  disconnect(): void {
    this.disconnectCalls += 1;
    this.statusHandler?.({ status: "disconnected" });
  }

  destroy(): void {
    this.destroyCalls += 1;
  }

  on(_event: "status", handler: (event: { status: "connected" | "disconnected" }) => void): void {
    this.statusHandler = handler;
  }
}

describe("createCollaborationProvider", () => {
  it("creates a CodeMirror collaboration extension", () => {
    const doc = new Y.Doc();
    const fakeProvider = new FakeProvider(doc);
    const provider = createCollaborationProvider({
      url: "ws://localhost:1234/yjs",
      room: "workspace:doc",
      doc,
      connectOnCreate: false,
      providerFactory: () => fakeProvider,
    });

    expect(provider.extension()).toBeDefined();
  });

  it("handles connect/disconnect/reconnect lifecycle", () => {
    const doc = new Y.Doc();
    const fakeProvider = new FakeProvider(doc);
    const provider = createCollaborationProvider({
      url: "ws://localhost:1234/yjs",
      room: "workspace:doc",
      doc,
      connectOnCreate: false,
      providerFactory: () => fakeProvider,
    });

    expect(provider.isConnected()).toBe(false);

    provider.connect();
    expect(provider.isConnected()).toBe(true);
    expect(fakeProvider.connectCalls).toBe(1);

    provider.disconnect();
    expect(provider.isConnected()).toBe(false);
    expect(fakeProvider.disconnectCalls).toBe(1);

    provider.reconnect();
    expect(provider.isConnected()).toBe(true);
    expect(fakeProvider.disconnectCalls).toBe(2);
    expect(fakeProvider.connectCalls).toBe(2);
  });

  it("connects automatically and destroys provider", () => {
    const doc = new Y.Doc();
    const fakeProvider = new FakeProvider(doc);
    const provider = createCollaborationProvider({
      url: "ws://localhost:1234/yjs",
      room: "workspace:doc",
      doc,
      providerFactory: () => fakeProvider,
    });

    expect(provider.isConnected()).toBe(true);
    expect(fakeProvider.connectCalls).toBe(1);

    provider.destroy();
    expect(provider.isConnected()).toBe(false);
    expect(fakeProvider.destroyCalls).toBe(1);
  });

  it("keeps relay transport active while enabling optional webrtc optimization", () => {
    const doc = new Y.Doc();
    const relayProvider = new FakeProvider(doc);
    const webrtcProvider = new FakeProvider(doc);
    const provider = createCollaborationProvider({
      url: "ws://localhost:1234/yjs",
      room: "workspace:doc",
      doc,
      connectOnCreate: false,
      providerFactory: () => relayProvider,
      webrtcSignalingUrl: "wss://signal.example.test",
      webrtcProviderFactory: () => webrtcProvider,
    });

    provider.connect();
    expect(provider.isConnected()).toBe(true);
    expect(relayProvider.connectCalls).toBe(1);
    expect(webrtcProvider.connectCalls).toBe(1);

    provider.disconnect();
    expect(provider.isConnected()).toBe(false);
    expect(relayProvider.disconnectCalls).toBe(1);
    expect(webrtcProvider.disconnectCalls).toBe(1);

    provider.destroy();
    expect(relayProvider.destroyCalls).toBe(1);
    expect(webrtcProvider.destroyCalls).toBe(1);
  });
});
