import "fake-indexeddb/auto";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { type IdbCrdtStore, openIdbCrdtStore } from "./idb-store";

let dbCounter = 0;

function createOpenFailureFactory(message: string): IDBFactory {
  return {
    open: () => {
      const request = {} as IDBOpenDBRequest;
      queueMicrotask(() => {
        (request as unknown as { error: DOMException | null }).error =
          new DOMException(message, "AbortError");
        request.onerror?.(new Event("error"));
      });
      return request;
    },
  } as unknown as IDBFactory;
}

function createAbortTransactionFactory(error: DOMException): IDBFactory {
  return {
    open: () => {
      const request = {} as IDBOpenDBRequest;
      queueMicrotask(() => {
        const mutableTransaction = {
          error: null as DOMException | null,
          onabort: null as ((event: Event) => void) | null,
          oncomplete: null as ((event: Event) => void) | null,
          onerror: null as ((event: Event) => void) | null,
          objectStore: () => ({
            put: () => {
              queueMicrotask(() => {
                mutableTransaction.error = error;
                mutableTransaction.onabort?.(new Event("abort"));
              });
            },
          }),
        };
        const transaction = mutableTransaction as unknown as IDBTransaction;
        const db = {
          close: () => undefined,
          transaction: () => transaction,
        } as unknown as IDBDatabase;
        (request as IDBOpenDBRequest & { result: IDBDatabase }).result = db;
        request.onsuccess?.(new Event("success"));
      });
      return request;
    },
  } as unknown as IDBFactory;
}

describe("IdbCrdtStore", () => {
  let store: IdbCrdtStore;

  beforeEach(async () => {
    dbCounter += 1;
    store = await openIdbCrdtStore({
      idbFactory: indexedDB,
      dbName: `test-crdt-${dbCounter}`,
    });
  });

  afterEach(() => {
    store.close();
  });

  // ── Snapshots ────────────────────────────────────────────────────

  it("returns null for a document with no snapshot", async () => {
    expect(await store.loadSnapshot("missing")).toBeNull();
  });

  it("saves and loads a snapshot", async () => {
    const state = new Uint8Array([1, 2, 3, 4]);
    await store.saveSnapshot("doc-a", state);

    const loaded = await store.loadSnapshot("doc-a");
    expect(loaded).toEqual(state);
  });

  it("overwrites a previous snapshot", async () => {
    await store.saveSnapshot("doc-a", new Uint8Array([1]));
    await store.saveSnapshot("doc-a", new Uint8Array([2, 3]));

    const loaded = await store.loadSnapshot("doc-a");
    expect(loaded).toEqual(new Uint8Array([2, 3]));
  });

  it("isolates snapshots between documents", async () => {
    await store.saveSnapshot("doc-a", new Uint8Array([10]));
    await store.saveSnapshot("doc-b", new Uint8Array([20]));

    expect(await store.loadSnapshot("doc-a")).toEqual(new Uint8Array([10]));
    expect(await store.loadSnapshot("doc-b")).toEqual(new Uint8Array([20]));
  });

  // ── Update queue ─────────────────────────────────────────────────

  it("returns empty array when no updates queued", async () => {
    expect(await store.getQueuedUpdates("doc-a")).toEqual([]);
  });

  it("queues and retrieves updates in order", async () => {
    await store.queueUpdate("doc-a", new Uint8Array([1]));
    await store.queueUpdate("doc-a", new Uint8Array([2]));
    await store.queueUpdate("doc-a", new Uint8Array([3]));

    const updates = await store.getQueuedUpdates("doc-a");
    expect(updates).toEqual([
      new Uint8Array([1]),
      new Uint8Array([2]),
      new Uint8Array([3]),
    ]);
  });

  it("isolates update queues between documents", async () => {
    await store.queueUpdate("doc-a", new Uint8Array([10]));
    await store.queueUpdate("doc-b", new Uint8Array([20]));

    expect(await store.getQueuedUpdates("doc-a")).toEqual([
      new Uint8Array([10]),
    ]);
    expect(await store.getQueuedUpdates("doc-b")).toEqual([
      new Uint8Array([20]),
    ]);
  });

  it("clears queued updates for a specific document", async () => {
    await store.queueUpdate("doc-a", new Uint8Array([1]));
    await store.queueUpdate("doc-b", new Uint8Array([2]));

    await store.clearQueuedUpdates("doc-a");

    expect(await store.getQueuedUpdates("doc-a")).toEqual([]);
    expect(await store.getQueuedUpdates("doc-b")).toEqual([
      new Uint8Array([2]),
    ]);
  });

  it("clearQueuedUpdates is a no-op when queue is empty", async () => {
    await store.clearQueuedUpdates("nonexistent");
    expect(await store.getQueuedUpdates("nonexistent")).toEqual([]);
  });

  // ── deleteDocument ───────────────────────────────────────────────

  it("deletes snapshot and queued updates for a document", async () => {
    await store.saveSnapshot("doc-a", new Uint8Array([1]));
    await store.queueUpdate("doc-a", new Uint8Array([2]));
    await store.queueUpdate("doc-a", new Uint8Array([3]));

    await store.deleteDocument("doc-a");

    expect(await store.loadSnapshot("doc-a")).toBeNull();
    expect(await store.getQueuedUpdates("doc-a")).toEqual([]);
  });

  it("deleteDocument does not affect other documents", async () => {
    await store.saveSnapshot("doc-a", new Uint8Array([10]));
    await store.saveSnapshot("doc-b", new Uint8Array([20]));
    await store.queueUpdate("doc-a", new Uint8Array([11]));
    await store.queueUpdate("doc-b", new Uint8Array([21]));

    await store.deleteDocument("doc-a");

    expect(await store.loadSnapshot("doc-b")).toEqual(new Uint8Array([20]));
    expect(await store.getQueuedUpdates("doc-b")).toEqual([
      new Uint8Array([21]),
    ]);
  });

  it("deleteDocument is a no-op for nonexistent documents", async () => {
    await store.deleteDocument("nonexistent");
  });

  it("supports concurrent access from multiple store instances", async () => {
    dbCounter += 1;
    const dbName = `test-crdt-concurrent-${dbCounter}`;
    const storeA = await openIdbCrdtStore({
      idbFactory: indexedDB,
      dbName,
    });
    const storeB = await openIdbCrdtStore({
      idbFactory: indexedDB,
      dbName,
    });

    try {
      await storeA.saveSnapshot("doc-a", new Uint8Array([9, 9]));
      expect(await storeB.loadSnapshot("doc-a")).toEqual(new Uint8Array([9, 9]));

      await Promise.all([
        storeA.queueUpdate("doc-a", new Uint8Array([1])),
        storeB.queueUpdate("doc-a", new Uint8Array([2])),
      ]);
      const queuedPayloads = (await storeA.getQueuedUpdates("doc-a"))
        .map((update) => Array.from(update).join(","))
        .sort();
      expect(queuedPayloads).toEqual(["1", "2"]);

      await storeB.clearQueuedUpdates("doc-a");
      expect(await storeA.getQueuedUpdates("doc-a")).toEqual([]);
    } finally {
      storeA.close();
      storeB.close();
    }
  });

  it("surfaces IndexedDB open failures", async () => {
    const idbFactory = createOpenFailureFactory("blocked");
    await expect(
      openIdbCrdtStore({ idbFactory, dbName: "test-crdt-open-failure" }),
    ).rejects.toThrow("Failed to open IndexedDB: blocked");
  });

  it("propagates transaction abort errors such as quota exceeded", async () => {
    const quotaError = new DOMException("Quota exceeded", "QuotaExceededError");
    const idbFactory = createAbortTransactionFactory(quotaError);
    const failingStore = await openIdbCrdtStore({
      idbFactory,
      dbName: "test-crdt-transaction-abort",
    });

    await expect(
      failingStore.saveSnapshot("doc-a", new Uint8Array([1])),
    ).rejects.toBe(quotaError);
    failingStore.close();
  });
});
