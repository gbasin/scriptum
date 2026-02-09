import type * as Y from "yjs";
import { create, type StoreApi, type UseBoundStore } from "zustand";
import { asNullableString, asNumber, asString } from "../lib/type-guards";

const DEFAULT_PRESENCE_MAP_NAME = "presence";

/** Cursor position within a document. */
export interface CursorPosition {
  /** Section ID the cursor is in. */
  sectionId: string | null;
  /** Line number within the section (0-based). */
  line: number;
  /** Column within the line (0-based). */
  column: number;
}

/** A single peer's presence state. */
export interface PeerPresence {
  /** Agent/user name. */
  name: string;
  /** Human or agent. */
  type: "human" | "agent";
  /** Document path the peer is viewing. */
  activeDocumentPath: string | null;
  /** Cursor position, if available. */
  cursor: CursorPosition | null;
  /** ISO timestamp of last heartbeat. */
  lastSeenAt: string;
  /** Hex colour for avatar/cursor highlight. */
  color: string;
}

interface PresenceSnapshot {
  peers: PeerPresence[];
  localPeerName: string | null;
}

interface ResolvedPresenceSnapshot extends PresenceSnapshot {
  /** Peers excluding the local user. */
  remotePeers: PeerPresence[];
  /** Number of online peers (including local). */
  onlineCount: number;
}

export interface PresenceStoreState extends ResolvedPresenceSnapshot {
  setPeers: (peers: PeerPresence[]) => void;
  upsertPeer: (peer: PeerPresence) => void;
  removePeer: (name: string) => void;
  setLocalPeerName: (name: string | null) => void;
  reset: () => void;
}

export type PresenceStore = UseBoundStore<StoreApi<PresenceStoreState>>;

export interface PresenceYjsBindingOptions {
  presenceMapName?: string;
  store?: PresenceStore;
}

function normalizeCursor(value: unknown): CursorPosition | null {
  if (!value || typeof value !== "object") {
    return null;
  }

  const cursor = value as Record<string, unknown>;
  const sectionId = asNullableString(cursor.sectionId);
  const line = asNumber(cursor.line);
  const column = asNumber(cursor.column);

  if (line === null || column === null) {
    return null;
  }

  return { sectionId, line, column };
}

function normalizePeer(value: unknown): PeerPresence | null {
  if (!value || typeof value !== "object") {
    return null;
  }

  const peer = value as Record<string, unknown>;
  const name = asString(peer.name);
  const type = asString(peer.type);
  const activeDocumentPath = asNullableString(peer.activeDocumentPath);
  const cursor = normalizeCursor(peer.cursor);
  const lastSeenAt = asString(peer.lastSeenAt);
  const color = asString(peer.color);

  if (
    !name ||
    (type !== "human" && type !== "agent") ||
    !lastSeenAt ||
    !color
  ) {
    return null;
  }

  return { name, type, activeDocumentPath, cursor, lastSeenAt, color };
}

function normalizePeers(values: readonly unknown[]): PeerPresence[] {
  const peers: PeerPresence[] = [];
  const seenNames = new Set<string>();

  for (const value of values) {
    const peer = normalizePeer(value);
    if (!peer || seenNames.has(peer.name)) {
      continue;
    }
    seenNames.add(peer.name);
    peers.push(peer);
  }

  return peers;
}

function resolvePresenceSnapshot(
  snapshot: PresenceSnapshot,
): ResolvedPresenceSnapshot {
  const peers = snapshot.peers.slice();
  const localPeerName = snapshot.localPeerName;
  const remotePeers = localPeerName
    ? peers.filter((peer) => peer.name !== localPeerName)
    : peers.slice();

  return {
    peers,
    localPeerName,
    remotePeers,
    onlineCount: peers.length,
  };
}

export function createPresenceStore(
  initial: Partial<PresenceSnapshot> = {},
): PresenceStore {
  return create<PresenceStoreState>()((set, get) => ({
    ...resolvePresenceSnapshot({
      peers: initial.peers ?? [],
      localPeerName: initial.localPeerName ?? null,
    }),
    setPeers: (peers) => {
      const previous = get();
      set(
        resolvePresenceSnapshot({
          peers,
          localPeerName: previous.localPeerName,
        }),
      );
    },
    upsertPeer: (peer) => {
      const previous = get();
      const index = previous.peers.findIndex(
        (candidate) => candidate.name === peer.name,
      );
      const peers =
        index >= 0
          ? previous.peers.map((candidate) =>
              candidate.name === peer.name ? peer : candidate,
            )
          : [...previous.peers, peer];

      set(
        resolvePresenceSnapshot({
          peers,
          localPeerName: previous.localPeerName,
        }),
      );
    },
    removePeer: (name) => {
      const previous = get();
      const peers = previous.peers.filter((peer) => peer.name !== name);
      set(
        resolvePresenceSnapshot({
          peers,
          localPeerName: previous.localPeerName,
        }),
      );
    },
    setLocalPeerName: (name) => {
      const previous = get();
      set(
        resolvePresenceSnapshot({
          peers: previous.peers,
          localPeerName: name,
        }),
      );
    },
    reset: () =>
      set(
        resolvePresenceSnapshot({
          peers: [],
          localPeerName: null,
        }),
      ),
  }));
}

export const usePresenceStore = createPresenceStore();

export function bindPresenceStoreToYjs(
  doc: Y.Doc,
  options: PresenceYjsBindingOptions = {},
): () => void {
  const store = options.store ?? usePresenceStore;
  const presenceMap = doc.getMap<unknown>(
    options.presenceMapName ?? DEFAULT_PRESENCE_MAP_NAME,
  );

  const syncFromYjs = () => {
    const entries: unknown[] = [];
    presenceMap.forEach((value) => {
      entries.push(value);
    });
    const peers = normalizePeers(entries);
    const localPeerName = store.getState().localPeerName;
    store.setState(resolvePresenceSnapshot({ peers, localPeerName }));
  };

  const handlePresenceChange = () => syncFromYjs();
  presenceMap.observe(handlePresenceChange);
  syncFromYjs();

  return () => {
    presenceMap.unobserve(handlePresenceChange);
  };
}
