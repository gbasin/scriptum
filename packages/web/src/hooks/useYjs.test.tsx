// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as Y from "yjs";
import type { IdbCrdtStore } from "../lib/idb-store";

const createCollaborationProviderMock = vi.hoisted(() => vi.fn());
const openIdbCrdtStoreMock = vi.hoisted(() => vi.fn());

vi.mock("@scriptum/editor", () => ({
  createCollaborationProvider: createCollaborationProviderMock,
}));

vi.mock("../lib/idb-store", () => ({
  openIdbCrdtStore: openIdbCrdtStoreMock,
}));

import { type UseYjsOptions, type UseYjsResult, useYjs } from "./useYjs";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

type ProviderStatus = "connected" | "disconnected";

class FakeSocketProvider {
  readonly awareness = {} as unknown as { __brand: "awareness" };

  private statusHandler: ((event: { status: ProviderStatus }) => void) | null =
    null;
  private connectImpl: () => void = () => {
    this.emitStatus("connected");
  };

  connect = vi.fn(() => {
    this.connectImpl();
  });

  disconnect = vi.fn(() => {
    this.emitStatus("disconnected");
  });

  destroy = vi.fn();

  on = vi.fn(
    (
      _event: "status",
      handler: (event: { status: ProviderStatus }) => void,
    ) => {
      this.statusHandler = handler;
    },
  );

  emitStatus(status: ProviderStatus): void {
    this.statusHandler?.({ status });
  }

  setConnectImpl(impl: () => void): void {
    this.connectImpl = impl;
  }
}

class FakeCollaborationProvider {
  readonly doc: Y.Doc;
  readonly yText: Y.Text;
  readonly provider = new FakeSocketProvider();

  connect = vi.fn(() => {
    this.provider.connect();
  });

  disconnect = vi.fn(() => {
    this.provider.disconnect();
  });

  destroy = vi.fn(() => {
    this.provider.destroy();
  });

  constructor(doc: Y.Doc) {
    this.doc = doc;
    this.yText = doc.getText("content");
  }
}

function createMockIdbStore(
  overrides: Partial<IdbCrdtStore> = {},
): IdbCrdtStore {
  return {
    saveSnapshot: vi.fn(async () => {}),
    loadSnapshot: vi.fn(async () => null),
    queueUpdate: vi.fn(async () => {}),
    getQueuedUpdates: vi.fn(async () => [] as Uint8Array[]),
    clearQueuedUpdates: vi.fn(async () => {}),
    deleteDocument: vi.fn(async () => {}),
    close: vi.fn(),
    ...overrides,
  };
}

async function flushAsyncWork(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

function renderUseYjs(options: UseYjsOptions) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  let latest: UseYjsResult | null = null;

  function Probe(props: { options: UseYjsOptions }) {
    latest = useYjs(props.options);
    return null;
  }

  act(() => {
    root.render(<Probe options={options} />);
  });

  return {
    latest: () => {
      if (!latest) {
        throw new Error("hook did not produce a value");
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

function requireProvider(
  provider: FakeCollaborationProvider | null,
): FakeCollaborationProvider {
  if (provider === null) {
    throw new Error("fakeProvider was not initialized");
  }
  return provider;
}

describe("useYjs", () => {
  beforeEach(() => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    createCollaborationProviderMock.mockReset();
    openIdbCrdtStoreMock.mockReset();
    openIdbCrdtStoreMock.mockResolvedValue(createMockIdbStore());
  });

  afterEach(() => {
    document.body.innerHTML = "";
    globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it("creates and connects a collaboration provider for the document", () => {
    let fakeProvider: FakeCollaborationProvider | null = null;
    createCollaborationProviderMock.mockImplementation(
      (input: { doc: Y.Doc }) => {
        fakeProvider = new FakeCollaborationProvider(input.doc);
        return fakeProvider as unknown as object;
      },
    );

    const harness = renderUseYjs({
      daemonWsUrl: "ws://127.0.0.1:39091/yjs",
      docId: "doc-alpha",
      runtime: "desktop",
      workspaceId: "ws-alpha",
    });
    const state = harness.latest();
    const provider = requireProvider(fakeProvider);

    expect(createCollaborationProviderMock).toHaveBeenCalledWith(
      expect.objectContaining({
        connectOnCreate: false,
        room: "ws-alpha:doc-alpha",
        url: "ws://127.0.0.1:39091/yjs",
      }),
    );
    expect(state.provider).toBe(provider);
    expect(state.ydoc).toBe(provider.doc);
    expect(state.ytext).toBe(provider.yText);
    expect(state.status).toBe("connected");
    expect(provider.connect).toHaveBeenCalledTimes(1);

    harness.unmount();
  });

  it("disconnects and destroys provider resources on unmount", () => {
    let fakeProvider: FakeCollaborationProvider | null = null;
    createCollaborationProviderMock.mockImplementation(
      (input: { doc: Y.Doc }) => {
        fakeProvider = new FakeCollaborationProvider(input.doc);
        return fakeProvider as unknown as object;
      },
    );

    const harness = renderUseYjs({
      daemonWsUrl: "ws://127.0.0.1:39091/yjs",
      docId: "doc-cleanup",
      runtime: "desktop",
    });
    const state = harness.latest();
    const docDestroySpy = vi.spyOn(state.ydoc as Y.Doc, "destroy");
    const provider = requireProvider(fakeProvider);

    harness.unmount();

    expect(provider.disconnect).toHaveBeenCalledTimes(1);
    expect(provider.destroy).toHaveBeenCalledTimes(1);
    expect(docDestroySpy).toHaveBeenCalledTimes(1);
  });

  it("reconnects after provider disconnects", () => {
    vi.useFakeTimers();

    let fakeProvider: FakeCollaborationProvider | null = null;
    createCollaborationProviderMock.mockImplementation(
      (input: { doc: Y.Doc }) => {
        fakeProvider = new FakeCollaborationProvider(input.doc);
        return fakeProvider as unknown as object;
      },
    );

    const harness = renderUseYjs({
      daemonWsUrl: "ws://127.0.0.1:39091/yjs",
      docId: "doc-reconnect",
      reconnectDelayMs: 25,
      runtime: "desktop",
    });
    const provider = requireProvider(fakeProvider);

    expect(provider.connect).toHaveBeenCalledTimes(1);
    expect(harness.latest().status).toBe("connected");

    act(() => {
      provider.provider.emitStatus("disconnected");
    });
    expect(harness.latest().status).toBe("disconnected");

    act(() => {
      vi.advanceTimersByTime(25);
    });
    expect(provider.connect).toHaveBeenCalledTimes(2);
    expect(harness.latest().status).toBe("connected");

    harness.unmount();
  });

  it("sets error status when provider connect throws", () => {
    let fakeProvider: FakeCollaborationProvider | null = null;
    createCollaborationProviderMock.mockImplementation(
      (input: { doc: Y.Doc }) => {
        fakeProvider = new FakeCollaborationProvider(input.doc);
        fakeProvider.provider.setConnectImpl(() => {
          throw new Error("connect failed");
        });
        return fakeProvider as unknown as object;
      },
    );

    const harness = renderUseYjs({
      daemonWsUrl: "ws://127.0.0.1:39091/yjs",
      docId: "doc-error",
      runtime: "desktop",
    });
    const provider = requireProvider(fakeProvider);

    expect(provider.connect).toHaveBeenCalledTimes(1);
    expect(harness.latest().status).toBe("error");

    harness.unmount();
  });

  it("uses relay websocket url for web runtime", () => {
    createCollaborationProviderMock.mockImplementation(
      (input: { doc: Y.Doc }) => {
        return new FakeCollaborationProvider(input.doc) as unknown as object;
      },
    );

    const harness = renderUseYjs({
      daemonWsUrl: "ws://127.0.0.1:39091/yjs",
      docId: "doc-web",
      relayWsUrl: "wss://relay.scriptum.dev/yjs",
      runtime: "web",
      workspaceId: "ws-web",
    });

    expect(createCollaborationProviderMock).toHaveBeenCalledWith(
      expect.objectContaining({
        room: "ws-web:doc-web",
        url: "wss://relay.scriptum.dev/yjs",
      }),
    );

    harness.unmount();
  });

  it("loads snapshot and queued updates before connecting in web runtime", async () => {
    let fakeProvider: FakeCollaborationProvider | null = null;
    createCollaborationProviderMock.mockImplementation(
      (input: { doc: Y.Doc }) => {
        fakeProvider = new FakeCollaborationProvider(input.doc);
        return fakeProvider as unknown as object;
      },
    );

    const persistedDoc = new Y.Doc();
    persistedDoc.getText("content").insert(0, "hello");
    const snapshot = Y.encodeStateAsUpdate(persistedDoc);
    const vectorAtSnapshot = Y.encodeStateVector(persistedDoc);
    persistedDoc.getText("content").insert(5, " world");
    const queuedUpdate = Y.encodeStateAsUpdate(persistedDoc, vectorAtSnapshot);

    const store = createMockIdbStore({
      loadSnapshot: vi.fn(async () => snapshot),
      getQueuedUpdates: vi.fn(async () => [queuedUpdate]),
    });
    openIdbCrdtStoreMock.mockResolvedValue(store);

    const harness = renderUseYjs({
      daemonWsUrl: "ws://127.0.0.1:39091/yjs",
      docId: "doc-web",
      runtime: "web",
      workspaceId: "ws-web",
    });
    await flushAsyncWork();

    const provider = requireProvider(fakeProvider);
    expect(store.loadSnapshot).toHaveBeenCalledWith("ws-web:doc-web");
    expect(store.getQueuedUpdates).toHaveBeenCalledWith("ws-web:doc-web");
    expect(provider.connect).toHaveBeenCalledTimes(1);
    expect(harness.latest().ytext?.toString()).toBe("hello world");

    harness.unmount();
  });

  it("queues Y.Doc updates for offline replay in web runtime", async () => {
    let fakeProvider: FakeCollaborationProvider | null = null;
    createCollaborationProviderMock.mockImplementation(
      (input: { doc: Y.Doc }) => {
        fakeProvider = new FakeCollaborationProvider(input.doc);
        return fakeProvider as unknown as object;
      },
    );

    const store = createMockIdbStore();
    openIdbCrdtStoreMock.mockResolvedValue(store);

    const harness = renderUseYjs({
      daemonWsUrl: "ws://127.0.0.1:39091/yjs",
      docId: "doc-queue",
      runtime: "web",
      workspaceId: "ws-web",
    });
    await flushAsyncWork();
    requireProvider(fakeProvider);

    const ytext = harness.latest().ytext;
    if (!ytext) {
      throw new Error("expected ytext to be initialized");
    }
    act(() => {
      ytext.insert(0, "queued");
    });
    await flushAsyncWork();

    expect(store.queueUpdate).toHaveBeenCalledTimes(1);
    expect(store.queueUpdate).toHaveBeenCalledWith(
      "ws-web:doc-queue",
      expect.any(Uint8Array),
    );

    harness.unmount();
  });

  it("clears queued updates and saves a snapshot after successful sync", async () => {
    createCollaborationProviderMock.mockImplementation(
      (input: { doc: Y.Doc }) => {
        return new FakeCollaborationProvider(input.doc) as unknown as object;
      },
    );

    const store = createMockIdbStore();
    openIdbCrdtStoreMock.mockResolvedValue(store);

    const harness = renderUseYjs({
      daemonWsUrl: "ws://127.0.0.1:39091/yjs",
      docId: "doc-sync",
      runtime: "web",
      workspaceId: "ws-web",
    });
    await flushAsyncWork();

    expect(store.clearQueuedUpdates).toHaveBeenCalledWith("ws-web:doc-sync");
    expect(store.saveSnapshot).toHaveBeenCalledWith(
      "ws-web:doc-sync",
      expect.any(Uint8Array),
    );

    harness.unmount();
  });

  it("replays queued updates when reconnecting in web runtime", async () => {
    vi.useFakeTimers();

    let fakeProvider: FakeCollaborationProvider | null = null;
    createCollaborationProviderMock.mockImplementation(
      (input: { doc: Y.Doc }) => {
        fakeProvider = new FakeCollaborationProvider(input.doc);
        return fakeProvider as unknown as object;
      },
    );

    const replayDoc = new Y.Doc();
    replayDoc.getText("content").insert(0, "offline-edit");
    const reconnectUpdate = Y.encodeStateAsUpdate(replayDoc);

    const getQueuedUpdatesMock = vi
      .fn<() => Promise<Uint8Array[]>>()
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([reconnectUpdate]);
    const store = createMockIdbStore({
      getQueuedUpdates: getQueuedUpdatesMock,
    });
    openIdbCrdtStoreMock.mockResolvedValue(store);

    const harness = renderUseYjs({
      daemonWsUrl: "ws://127.0.0.1:39091/yjs",
      docId: "doc-replay",
      reconnectDelayMs: 25,
      runtime: "web",
      workspaceId: "ws-web",
    });
    await flushAsyncWork();

    const provider = requireProvider(fakeProvider);
    expect(provider.connect).toHaveBeenCalledTimes(1);

    act(() => {
      provider.provider.emitStatus("disconnected");
    });
    expect(harness.latest().status).toBe("disconnected");

    await act(async () => {
      vi.advanceTimersByTime(25);
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(provider.connect).toHaveBeenCalledTimes(2);
    expect(store.getQueuedUpdates).toHaveBeenCalledTimes(2);
    expect(harness.latest().ytext?.toString()).toBe("offline-edit");

    harness.unmount();
  });

  it("closes the IndexedDB store on unmount in web runtime", async () => {
    createCollaborationProviderMock.mockImplementation(
      (input: { doc: Y.Doc }) => {
        return new FakeCollaborationProvider(input.doc) as unknown as object;
      },
    );

    const store = createMockIdbStore();
    openIdbCrdtStoreMock.mockResolvedValue(store);

    const harness = renderUseYjs({
      daemonWsUrl: "ws://127.0.0.1:39091/yjs",
      docId: "doc-close",
      runtime: "web",
      workspaceId: "ws-web",
    });
    await flushAsyncWork();

    harness.unmount();
    await flushAsyncWork();
    expect(store.close).toHaveBeenCalledTimes(1);
  });
});
