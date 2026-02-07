export type ShareLinkPermission = "view" | "edit";
export type ShareLinkTargetType = "workspace" | "document";
export type ShareLinkExpirationOption = "none" | "24h" | "7d";

export interface ShareLinkRecord {
  readonly token: string;
  readonly targetType: ShareLinkTargetType;
  readonly targetId: string;
  readonly permission: ShareLinkPermission;
  readonly expiresAt: string | null;
  readonly maxUses: number | null;
  readonly useCount: number;
  readonly disabled: boolean;
  readonly createdAt: string;
}

const SHARE_LINK_STORAGE_PREFIX = "scriptum-share-link:";

function storageKey(token: string): string {
  return `${SHARE_LINK_STORAGE_PREFIX}${token}`;
}

function coerceRecord(value: unknown): ShareLinkRecord | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }

  const record = value as Record<string, unknown>;
  const token = record.token;
  const targetType = record.targetType;
  const targetId = record.targetId;
  const permission = record.permission;
  const expiresAt = record.expiresAt;
  const maxUses = record.maxUses;
  const useCount = record.useCount;
  const disabled = record.disabled;
  const createdAt = record.createdAt;

  if (typeof token !== "string" || token.length === 0) {
    return null;
  }
  if (targetType !== "workspace" && targetType !== "document") {
    return null;
  }
  if (typeof targetId !== "string" || targetId.length === 0) {
    return null;
  }
  if (permission !== "view" && permission !== "edit") {
    return null;
  }
  if (expiresAt !== null && typeof expiresAt !== "string") {
    return null;
  }
  if (
    maxUses !== null &&
    (typeof maxUses !== "number" || !Number.isInteger(maxUses) || maxUses < 1)
  ) {
    return null;
  }
  if (
    typeof useCount !== "number" ||
    !Number.isInteger(useCount) ||
    useCount < 0
  ) {
    return null;
  }
  if (typeof disabled !== "boolean") {
    return null;
  }
  if (typeof createdAt !== "string") {
    return null;
  }

  return {
    token,
    targetType,
    targetId,
    permission,
    expiresAt,
    maxUses: maxUses === null ? null : maxUses,
    useCount,
    disabled,
    createdAt,
  };
}

export function buildShareLinkUrl(token: string, origin: string): string {
  const normalizedOrigin = origin.replace(/\/+$/, "");
  return `${normalizedOrigin}/share/${encodeURIComponent(token)}`;
}

export function sharePermissionLabel(permission: ShareLinkPermission): string {
  return permission === "edit" ? "editor" : "viewer";
}

export function expirationIsoFromOption(
  option: ShareLinkExpirationOption,
  nowMs = Date.now(),
): string | null {
  if (option === "none") {
    return null;
  }

  const nextMs =
    option === "24h" ? nowMs + 24 * 60 * 60 * 1000 : nowMs + 7 * 24 * 60 * 60 * 1000;
  return new Date(nextMs).toISOString();
}

export function parseShareLinkMaxUses(rawValue: string): number | null {
  const trimmed = rawValue.trim();
  if (!trimmed) {
    return null;
  }

  const parsed = Number.parseInt(trimmed, 10);
  if (!Number.isInteger(parsed) || parsed < 1) {
    return null;
  }
  return parsed;
}

export function createShareLinkRecord(input: {
  token: string;
  targetType: ShareLinkTargetType;
  targetId: string;
  permission: ShareLinkPermission;
  expiresAt: string | null;
  maxUses: number | null;
  nowMs?: number;
}): ShareLinkRecord {
  return {
    token: input.token,
    targetType: input.targetType,
    targetId: input.targetId,
    permission: input.permission,
    expiresAt: input.expiresAt,
    maxUses: input.maxUses,
    useCount: 0,
    disabled: false,
    createdAt: new Date(input.nowMs ?? Date.now()).toISOString(),
  };
}

export function storeShareLinkRecord(record: ShareLinkRecord): void {
  if (typeof window === "undefined") {
    return;
  }

  window.sessionStorage.setItem(storageKey(record.token), JSON.stringify(record));
}

export function loadShareLinkRecord(token: string): ShareLinkRecord | null {
  if (typeof window === "undefined") {
    return null;
  }

  const raw = window.sessionStorage.getItem(storageKey(token));
  if (!raw) {
    return null;
  }

  try {
    return coerceRecord(JSON.parse(raw));
  } catch {
    return null;
  }
}

export function isShareLinkRedeemable(
  record: ShareLinkRecord,
  nowMs = Date.now(),
): boolean {
  if (record.disabled) {
    return false;
  }
  if (record.expiresAt && Date.parse(record.expiresAt) <= nowMs) {
    return false;
  }
  if (record.maxUses !== null && record.useCount >= record.maxUses) {
    return false;
  }
  return true;
}

export function redeemShareLink(
  token: string,
  nowMs = Date.now(),
): ShareLinkRecord | null {
  const current = loadShareLinkRecord(token);
  if (!current || !isShareLinkRedeemable(current, nowMs)) {
    return null;
  }

  const next: ShareLinkRecord = {
    ...current,
    useCount: current.useCount + 1,
  };
  storeShareLinkRecord(next);
  return next;
}
