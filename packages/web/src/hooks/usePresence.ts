import {
  nameToColor,
  parseAwarenessPeer,
  readAwarenessPeers,
  type AwarenessPeerSnapshot,
} from "@scriptum/editor";
import { useCallback, useEffect, useState } from "react";

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

function toPresencePeer(peer: AwarenessPeerSnapshot): PresencePeer {
  return {
    clientId: peer.clientId,
    color: peer.color,
    cursor: peer.cursor
      ? {
          anchor: peer.cursor.anchor,
          head: peer.cursor.head,
          ...(peer.cursor.line === undefined ? {} : { line: peer.cursor.line }),
          ...(peer.cursor.column === undefined
            ? {}
            : { column: peer.cursor.column }),
          ...(peer.cursor.sectionId === undefined
            ? {}
            : { sectionId: peer.cursor.sectionId }),
        }
      : null,
    name: peer.name,
    type: peer.type,
  };
}

function readPeers(awareness: AwarenessLike): ParsedPeers {
  const localClientId = awareness.clientID;
  const awarenessPeers = readAwarenessPeers(awareness.getStates(), {
    fallbackColor: nameToColor,
  });
  const connectedPeers: PresencePeer[] = [];
  let localPeer: PresencePeer | null = null;

  for (const peerState of awarenessPeers) {
    const peer = toPresencePeer(peerState);
    if (peer.clientId === localClientId) {
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

      const localPeer = parseAwarenessPeer(
        awareness.clientID,
        awareness.getLocalState(),
        nameToColor,
      );

      awareness.setLocalStateField("user", {
        color: localPeer.color,
        name: localPeer.name,
        type: localPeer.type,
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
