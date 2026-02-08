import {
  type CollaborationProvider,
  createCollaborationProvider,
  type ProviderFactory,
  type WebRtcProviderFactory,
} from "@scriptum/editor";
import { useEffect, useMemo, useState } from "react";
import * as Y from "yjs";
import { type IdbCrdtStore, openIdbCrdtStore } from "../lib/idb-store";

const DEFAULT_DAEMON_WS_URL = "ws://127.0.0.1:39091/yjs";
const DEFAULT_RECONNECT_DELAY_MS = 1_000;
const PERSISTENCE_REPLAY_ORIGIN = "idb-persistence-replay";

export type YjsRuntime = "desktop" | "web";
export type YjsProviderStatus =
  | "connecting"
  | "connected"
  | "disconnected"
  | "error";

export interface UseYjsOptions {
  docId: string;
  workspaceId?: string;
  runtime?: YjsRuntime;
  daemonWsUrl?: string;
  relayWsUrl?: string;
  reconnectDelayMs?: number;
  enabled?: boolean;
  providerFactory?: ProviderFactory;
  webrtcSignalingUrl?: string | null;
  webrtcProviderFactory?: WebRtcProviderFactory;
}

export interface UseYjsResult {
  ytext: Y.Text | null;
  ydoc: Y.Doc | null;
  provider: CollaborationProvider | null;
  status: YjsProviderStatus;
}

function defaultDaemonWsUrl(): string {
  return (
    (import.meta.env.VITE_SCRIPTUM_DAEMON_WS_URL as string | undefined) ??
    DEFAULT_DAEMON_WS_URL
  );
}

function defaultRelayWsUrl(daemonWsUrl: string): string {
  const explicit = (
    import.meta.env.VITE_SCRIPTUM_RELAY_WS_URL as string | undefined
  )?.trim();
  if (explicit) {
    return explicit;
  }

  const relayHttpUrl = (
    import.meta.env.VITE_SCRIPTUM_RELAY_URL as string | undefined
  )?.trim();
  if (!relayHttpUrl) {
    return daemonWsUrl;
  }

  try {
    const parsed = new URL(relayHttpUrl);
    parsed.protocol = parsed.protocol === "https:" ? "wss:" : "ws:";
    if (parsed.pathname === "/" || parsed.pathname === "") {
      parsed.pathname = "/yjs";
    }
    return parsed.toString().replace(/\/$/, "");
  } catch {
    return daemonWsUrl;
  }
}

function roomFromIds(workspaceId: string | undefined, docId: string): string {
  return workspaceId ? `${workspaceId}:${docId}` : docId;
}

function providerUrl(
  runtime: YjsRuntime,
  daemonWsUrl: string,
  relayWsUrl: string,
): string {
  return runtime === "web" ? relayWsUrl : daemonWsUrl;
}

export function useYjs(options: UseYjsOptions): UseYjsResult {
  const {
    docId,
    workspaceId,
    runtime = "desktop",
    daemonWsUrl = defaultDaemonWsUrl(),
    relayWsUrl = defaultRelayWsUrl(daemonWsUrl),
    reconnectDelayMs = DEFAULT_RECONNECT_DELAY_MS,
    enabled = true,
    providerFactory,
    webrtcSignalingUrl,
    webrtcProviderFactory,
  } = options;

  const room = useMemo(
    () => roomFromIds(workspaceId, docId),
    [docId, workspaceId],
  );
  const url = useMemo(
    () => providerUrl(runtime, daemonWsUrl, relayWsUrl),
    [daemonWsUrl, relayWsUrl, runtime],
  );

  const [provider, setProvider] = useState<CollaborationProvider | null>(null);
  const [ydoc, setYdoc] = useState<Y.Doc | null>(null);
  const [ytext, setYtext] = useState<Y.Text | null>(null);
  const [status, setStatus] = useState<YjsProviderStatus>("disconnected");

  useEffect(() => {
    if (!enabled || docId.trim().length === 0) {
      setProvider(null);
      setYdoc(null);
      setYtext(null);
      setStatus("disconnected");
      return;
    }

    let disposed = false;
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
    let persistenceStore: IdbCrdtStore | null = null;
    let persistenceQueue: Promise<void> = Promise.resolve();
    const persistenceEnabled = runtime === "web";

    const doc = new Y.Doc();
    const collaboration = createCollaborationProvider({
      connectOnCreate: false,
      doc,
      room,
      url,
      providerFactory,
      webrtcSignalingUrl: webrtcSignalingUrl ?? undefined,
      webrtcProviderFactory,
    });

    const runPersistenceTask = (
      task: (store: IdbCrdtStore) => Promise<void>,
    ) => {
      if (persistenceStore === null) {
        return;
      }
      persistenceQueue = persistenceQueue
        .then(async () => {
          if (disposed || persistenceStore === null) {
            return;
          }
          await task(persistenceStore);
        })
        .catch(() => {});
    };

    const queueDocUpdate = (update: Uint8Array, origin: unknown) => {
      if (
        persistenceStore === null ||
        origin === PERSISTENCE_REPLAY_ORIGIN ||
        disposed
      ) {
        return;
      }
      const updateCopy = update.slice();
      runPersistenceTask(async (store) => {
        await store.queueUpdate(room, updateCopy);
      });
    };

    const replayQueuedUpdates = async () => {
      if (persistenceStore === null) {
        return;
      }
      const updates = await persistenceStore.getQueuedUpdates(room);
      for (const update of updates) {
        Y.applyUpdate(doc, update, PERSISTENCE_REPLAY_ORIGIN);
      }
    };

    const persistSyncedSnapshot = () => {
      if (persistenceStore === null || disposed) {
        return;
      }
      const snapshot = Y.encodeStateAsUpdate(doc);
      runPersistenceTask(async (store) => {
        await store.clearQueuedUpdates(room);
        await store.saveSnapshot(room, snapshot);
      });
    };

    const clearReconnectTimer = () => {
      if (reconnectTimer !== null) {
        clearTimeout(reconnectTimer);
        reconnectTimer = null;
      }
    };

    const scheduleReconnect = () => {
      if (disposed || reconnectTimer !== null) {
        return;
      }
      reconnectTimer = setTimeout(() => {
        reconnectTimer = null;
        void connectProvider();
      }, reconnectDelayMs);
    };

    const connectProvider = async () => {
      if (disposed) {
        return;
      }
      setStatus("connecting");
      try {
        if (persistenceEnabled) {
          await replayQueuedUpdates();
        }
        if (disposed) {
          return;
        }
        collaboration.connect();
      } catch {
        if (disposed) {
          return;
        }
        setStatus("error");
        scheduleReconnect();
      }
    };

    collaboration.provider.on("status", ({ status: nextStatus }) => {
      if (disposed) {
        return;
      }
      if (nextStatus === "connected") {
        clearReconnectTimer();
        setStatus("connected");
        persistSyncedSnapshot();
        return;
      }
      setStatus("disconnected");
      scheduleReconnect();
    });

    const initialize = async () => {
      if (persistenceEnabled) {
        try {
          persistenceStore = await openIdbCrdtStore();
          const snapshot = await persistenceStore.loadSnapshot(room);
          if (snapshot !== null) {
            Y.applyUpdate(doc, snapshot, PERSISTENCE_REPLAY_ORIGIN);
          }
          doc.on("update", queueDocUpdate);
        } catch {
          persistenceStore = null;
        }
      }

      if (disposed) {
        return;
      }

      setProvider(collaboration);
      setYdoc(doc);
      setYtext(collaboration.yText);
      void connectProvider();
    };

    void initialize();

    return () => {
      disposed = true;
      clearReconnectTimer();
      doc.off("update", queueDocUpdate);
      const storeToClose = persistenceStore;
      persistenceStore = null;
      if (storeToClose !== null) {
        void persistenceQueue.finally(() => {
          storeToClose.close();
        });
      }
      collaboration.disconnect();
      collaboration.destroy();
      doc.destroy();
    };
  }, [
    docId,
    enabled,
    providerFactory,
    runtime,
    reconnectDelayMs,
    room,
    url,
    webrtcProviderFactory,
    webrtcSignalingUrl,
  ]);

  return { ydoc, ytext, provider, status };
}
