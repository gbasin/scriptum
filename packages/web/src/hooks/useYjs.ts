import {
  type CollaborationProvider,
  createCollaborationProvider,
  type ProviderFactory,
  type WebRtcProviderFactory,
} from "@scriptum/editor";
import { useEffect, useMemo, useState } from "react";
import * as Y from "yjs";

const DEFAULT_DAEMON_WS_URL = "ws://127.0.0.1:39091/yjs";
const DEFAULT_RECONNECT_DELAY_MS = 1_000;

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
        connectProvider();
      }, reconnectDelayMs);
    };

    const connectProvider = () => {
      if (disposed) {
        return;
      }
      setStatus("connecting");
      try {
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
        return;
      }
      setStatus("disconnected");
      scheduleReconnect();
    });

    setProvider(collaboration);
    setYdoc(doc);
    setYtext(collaboration.yText);
    connectProvider();

    return () => {
      disposed = true;
      clearReconnectTimer();
      collaboration.disconnect();
      collaboration.destroy();
      doc.destroy();
    };
  }, [
    docId,
    enabled,
    providerFactory,
    reconnectDelayMs,
    room,
    url,
    webrtcProviderFactory,
    webrtcSignalingUrl,
  ]);

  return { ydoc, ytext, provider, status };
}
