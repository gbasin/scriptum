import { nameToColor } from "@scriptum/editor";
import { useCallback, useEffect, useState } from "react";
import { asNumber, asRecord } from "../lib/type-guards";

export interface PresenceCursor {
  anchor: number;
  head: number;
  line?: number;
  column?: number;
  sectionId?: string | null;
}

export type PresenceViewport = Record<string, unknown> | null;

export interface PresencePeer {
  clientId: number;
  name: string;
  type: "human" | "agent";
  cursor: PresenceCursor | null;
  color: string;
}

export interface UsePresenceOptions {
  awareness: AwarenessLike | null;
}

export interface UsePresenceResult {
  connectedPeers: PresencePeer[];
  localPeer: PresencePeer | null;
  setLocalState: (
    cursor: PresenceCursor | null,
    viewport?: PresenceViewport,
  ) => void;
}

interface ParsedPeers {
  connectedPeers: PresencePeer[];
  localPeer: PresencePeer | null;
}

export interface AwarenessLike {
  readonly clientID: number;
  getLocalState(): unknown;
  getStates(): Map<number, unknown>;
  on(event: "change", listener: () => void): void;
  off(event: "change", listener: () => void): void;
  setLocalStateField(field: string, value: unknown): void;
}

function normalizeCursor(value: unknown): PresenceCursor | null {
  const cursor = asRecord(value);
  if (!cursor) {
    return null;
  }

  const anchor = asNumber(cursor.anchor);
  const head = asNumber(cursor.head);
  if (anchor === null || head === null) {
    return null;
  }

  const sectionIdRaw = cursor.sectionId;
  const sectionId =
    typeof sectionIdRaw === "string" || sectionIdRaw === null
      ? sectionIdRaw
      : undefined;
  const line = asNumber(cursor.line) ?? undefined;
  const column = asNumber(cursor.column) ?? undefined;

  return {
    anchor,
    column,
    head,
    line,
    sectionId,
  };
}

function toPeer(clientId: number, state: unknown): PresencePeer {
  const stateRecord = asRecord(state);
  const user = asRecord(stateRecord?.user);

  const name =
    typeof user?.name === "string" && user.name.trim().length > 0
      ? user.name
      : `User ${clientId}`;
  const type = user?.type === "agent" ? "agent" : "human";
  const color =
    typeof user?.color === "string" && user.color.trim().length > 0
      ? user.color
      : nameToColor(name);

  return {
    clientId,
    color,
    cursor: normalizeCursor(stateRecord?.cursor),
    name,
    type,
  };
}

function readPeers(awareness: AwarenessLike): ParsedPeers {
  const localClientId = awareness.clientID;
  const connectedPeers: PresencePeer[] = [];
  let localPeer: PresencePeer | null = null;

  for (const [clientId, state] of awareness.getStates()) {
    const peer = toPeer(clientId, state);
    if (clientId === localClientId) {
      localPeer = peer;
      continue;
    }
    connectedPeers.push(peer);
  }

  connectedPeers.sort((left, right) => left.clientId - right.clientId);
  return { connectedPeers, localPeer };
}

const EMPTY_PEERS: ParsedPeers = {
  connectedPeers: [],
  localPeer: null,
};

export function usePresence(options: UsePresenceOptions): UsePresenceResult {
  const awareness = options.awareness;
  const [peers, setPeers] = useState<ParsedPeers>(EMPTY_PEERS);

  useEffect(() => {
    if (!awareness) {
      setPeers(EMPTY_PEERS);
      return;
    }

    const syncPeers = () => {
      setPeers(readPeers(awareness));
    };

    awareness.on("change", syncPeers);
    syncPeers();

    return () => {
      awareness.off("change", syncPeers);
    };
  }, [awareness]);

  const setLocalState = useCallback(
    (cursor: PresenceCursor | null, viewport: PresenceViewport = null) => {
      if (!awareness) {
        return;
      }

      const localState = asRecord(awareness.getLocalState());
      const user = asRecord(localState?.user);
      const localName =
        typeof user?.name === "string" && user.name.trim().length > 0
          ? user.name
          : `User ${awareness.clientID}`;
      const localType = user?.type === "agent" ? "agent" : "human";
      const color =
        typeof user?.color === "string" && user.color.trim().length > 0
          ? user.color
          : nameToColor(localName);

      awareness.setLocalStateField("user", {
        color,
        name: localName,
        type: localType,
      });
      awareness.setLocalStateField("cursor", cursor);
      awareness.setLocalStateField("viewport", viewport);
    },
    [awareness],
  );

  return {
    connectedPeers: peers.connectedPeers,
    localPeer: peers.localPeer,
    setLocalState,
  };
}
