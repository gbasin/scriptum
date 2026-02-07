// IndexedDB persistence for Y.Doc CRDT state.
// Stores full snapshots and queues incremental updates for offline replay.

const DB_NAME = "scriptum-crdt";
const DB_VERSION = 1;
const SNAPSHOT_STORE = "snapshots";
const UPDATE_QUEUE_STORE = "updates";

/** A stored Y.Doc snapshot. */
interface SnapshotRecord {
  /** Document identifier (workspace:doc). */
  docId: string;
  /** Full encoded state (Y.encodeStateAsUpdate output). */
  state: Uint8Array;
  /** ISO timestamp of when this snapshot was saved. */
  savedAt: string;
}

/** A queued incremental update (buffered while offline). */
interface UpdateRecord {
  /** Auto-incremented key. */
  id?: number;
  /** Document identifier (workspace:doc). */
  docId: string;
  /** Incremental Y.Doc update bytes. */
  update: Uint8Array;
  /** ISO timestamp of when the update was queued. */
  queuedAt: string;
}

export interface IdbCrdtStore {
  /** Persist a full Y.Doc snapshot (replaces any previous snapshot). */
  saveSnapshot(docId: string, state: Uint8Array): Promise<void>;
  /** Load the latest snapshot for a document. Returns null if none exists. */
  loadSnapshot(docId: string): Promise<Uint8Array | null>;
  /** Queue an incremental update for later sync. */
  queueUpdate(docId: string, update: Uint8Array): Promise<void>;
  /** Retrieve all queued updates for a document, ordered by queue time. */
  getQueuedUpdates(docId: string): Promise<Uint8Array[]>;
  /** Clear queued updates after successful sync. */
  clearQueuedUpdates(docId: string): Promise<void>;
  /** Delete all data (snapshot + queued updates) for a document. */
  deleteDocument(docId: string): Promise<void>;
  /** Close the database connection. */
  close(): void;
}

export interface IdbCrdtStoreOptions {
  /** Override the IDBFactory (for testing). Defaults to `globalThis.indexedDB`. */
  idbFactory?: IDBFactory;
  /** Override the database name (for test isolation). Defaults to "scriptum-crdt". */
  dbName?: string;
}

/**
 * Open (or create) the IndexedDB database and return an IdbCrdtStore.
 */
export function openIdbCrdtStore(
  options: IdbCrdtStoreOptions = {},
): Promise<IdbCrdtStore> {
  const factory = options.idbFactory ?? globalThis.indexedDB;
  const name = options.dbName ?? DB_NAME;

  return new Promise<IdbCrdtStore>((resolve, reject) => {
    const request = factory.open(name, DB_VERSION);

    request.onupgradeneeded = () => {
      const db = request.result;

      if (!db.objectStoreNames.contains(SNAPSHOT_STORE)) {
        db.createObjectStore(SNAPSHOT_STORE, { keyPath: "docId" });
      }

      if (!db.objectStoreNames.contains(UPDATE_QUEUE_STORE)) {
        const updateStore = db.createObjectStore(UPDATE_QUEUE_STORE, {
          keyPath: "id",
          autoIncrement: true,
        });
        updateStore.createIndex("byDocId", "docId", { unique: false });
      }
    };

    request.onsuccess = () => {
      resolve(createStore(request.result));
    };

    request.onerror = () => {
      reject(new Error(`Failed to open IndexedDB: ${request.error?.message}`));
    };
  });
}

function createStore(db: IDBDatabase): IdbCrdtStore {
  function tx(
    storeNames: string | string[],
    mode: IDBTransactionMode,
  ): IDBTransaction {
    return db.transaction(storeNames, mode);
  }

  function wrapRequest<T>(request: IDBRequest<T>): Promise<T> {
    return new Promise((resolve, reject) => {
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
  }

  function wrapTransaction(transaction: IDBTransaction): Promise<void> {
    return new Promise((resolve, reject) => {
      transaction.oncomplete = () => resolve();
      transaction.onerror = () => reject(transaction.error);
      transaction.onabort = () =>
        reject(transaction.error ?? new Error("Transaction aborted"));
    });
  }

  return {
    async saveSnapshot(docId: string, state: Uint8Array): Promise<void> {
      const transaction = tx(SNAPSHOT_STORE, "readwrite");
      const store = transaction.objectStore(SNAPSHOT_STORE);
      const record: SnapshotRecord = {
        docId,
        state,
        savedAt: new Date().toISOString(),
      };
      store.put(record);
      await wrapTransaction(transaction);
    },

    async loadSnapshot(docId: string): Promise<Uint8Array | null> {
      const transaction = tx(SNAPSHOT_STORE, "readonly");
      const store = transaction.objectStore(SNAPSHOT_STORE);
      const result = await wrapRequest<SnapshotRecord | undefined>(
        store.get(docId),
      );
      return result?.state ?? null;
    },

    async queueUpdate(docId: string, update: Uint8Array): Promise<void> {
      const transaction = tx(UPDATE_QUEUE_STORE, "readwrite");
      const store = transaction.objectStore(UPDATE_QUEUE_STORE);
      const record: UpdateRecord = {
        docId,
        update,
        queuedAt: new Date().toISOString(),
      };
      store.add(record);
      await wrapTransaction(transaction);
    },

    async getQueuedUpdates(docId: string): Promise<Uint8Array[]> {
      const transaction = tx(UPDATE_QUEUE_STORE, "readonly");
      const store = transaction.objectStore(UPDATE_QUEUE_STORE);
      const index = store.index("byDocId");
      const results = await wrapRequest<UpdateRecord[]>(index.getAll(docId));
      // Already ordered by auto-increment key.
      return results.map((record) => record.update);
    },

    async clearQueuedUpdates(docId: string): Promise<void> {
      const transaction = tx(UPDATE_QUEUE_STORE, "readwrite");
      const store = transaction.objectStore(UPDATE_QUEUE_STORE);
      const index = store.index("byDocId");
      const keys = await wrapRequest<IDBValidKey[]>(index.getAllKeys(docId));
      for (const key of keys) {
        store.delete(key);
      }
      await wrapTransaction(transaction);
    },

    async deleteDocument(docId: string): Promise<void> {
      const transaction = tx([SNAPSHOT_STORE, UPDATE_QUEUE_STORE], "readwrite");
      const snapshotStore = transaction.objectStore(SNAPSHOT_STORE);
      const updateStore = transaction.objectStore(UPDATE_QUEUE_STORE);
      const index = updateStore.index("byDocId");

      snapshotStore.delete(docId);

      const keys = await wrapRequest<IDBValidKey[]>(index.getAllKeys(docId));
      for (const key of keys) {
        updateStore.delete(key);
      }

      await wrapTransaction(transaction);
    },

    close(): void {
      db.close();
    },
  };
}
