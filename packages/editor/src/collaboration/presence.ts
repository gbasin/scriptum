export interface AwarenessPeerCursor {
  anchor: number;
  head: number;
  line?: number;
  column?: number;
  sectionId?: string | null;
}

export interface AwarenessPeerSnapshot {
  clientId: number;
  color: string;
  cursor: AwarenessPeerCursor | null;
  name: string;
  type: "human" | "agent";
}

export interface ReadAwarenessPeersOptions {
  fallbackColor: (name: string) => string;
  includeLocal?: boolean;
  localClientId?: number;
  sortByClientId?: boolean;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
}

function asNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

export function parseAwarenessPeer(
  clientId: number,
  state: unknown,
  fallbackColor: (name: string) => string,
): AwarenessPeerSnapshot {
  const stateRecord = asRecord(state);
  const user = asRecord(stateRecord?.user);
  const cursorRecord = asRecord(stateRecord?.cursor);

  const name =
    typeof user?.name === "string" && user.name.trim().length > 0
      ? user.name
      : `User ${clientId}`;
  const type = user?.type === "agent" ? "agent" : "human";
  const color =
    typeof user?.color === "string" && user.color.trim().length > 0
      ? user.color
      : fallbackColor(name);

  let cursor: AwarenessPeerCursor | null = null;
  if (cursorRecord) {
    const anchor = asNumber(cursorRecord.anchor);
    const head = asNumber(cursorRecord.head);
    if (anchor !== null || head !== null) {
      const safeAnchor = anchor ?? head ?? 0;
      const safeHead = head ?? anchor ?? 0;
      const line = asNumber(cursorRecord.line) ?? undefined;
      const column = asNumber(cursorRecord.column) ?? undefined;
      const sectionIdRaw = cursorRecord.sectionId;
      const sectionId =
        typeof sectionIdRaw === "string" || sectionIdRaw === null
          ? sectionIdRaw
          : undefined;
      cursor = {
        anchor: safeAnchor,
        head: safeHead,
        ...(line === undefined ? {} : { line }),
        ...(column === undefined ? {} : { column }),
        ...(sectionId === undefined ? {} : { sectionId }),
      };
    }
  }

  return {
    clientId,
    color,
    cursor,
    name,
    type,
  };
}

export function readAwarenessPeers(
  states: Iterable<[number, unknown]>,
  options: ReadAwarenessPeersOptions,
): AwarenessPeerSnapshot[] {
  const includeLocal = options.includeLocal ?? true;
  const sortByClientId = options.sortByClientId ?? true;
  const peers: AwarenessPeerSnapshot[] = [];

  for (const [clientId, state] of states) {
    if (!includeLocal && clientId === options.localClientId) {
      continue;
    }
    peers.push(parseAwarenessPeer(clientId, state, options.fallbackColor));
  }

  if (sortByClientId) {
    peers.sort((left, right) => left.clientId - right.clientId);
  }
  return peers;
}
