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

export interface RelayShareLinkRecord {
  readonly id: string;
  readonly targetType: ShareLinkTargetType;
  readonly targetId: string;
  readonly permission: ShareLinkPermission;
  readonly expiresAt: string | null;
  readonly maxUses: number | null;
  readonly useCount: number;
  readonly disabled: boolean;
  readonly createdAt: string;
  readonly revokedAt: string | null;
  readonly etag: string;
  readonly urlOnce: string;
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

function readApiString(
  record: Record<string, unknown>,
  keys: readonly string[],
): string | null {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string") {
      return value;
    }
  }
  return null;
}

function readApiNullableString(
  record: Record<string, unknown>,
  keys: readonly string[],
): string | null | undefined {
  for (const key of keys) {
    if (!(key in record)) {
      continue;
    }
    const value = record[key];
    if (value === null) {
      return null;
    }
    if (typeof value === "string") {
      return value;
    }
  }
  return undefined;
}

function readApiNullableInteger(
  record: Record<string, unknown>,
  keys: readonly string[],
): number | null | undefined {
  for (const key of keys) {
    if (!(key in record)) {
      continue;
    }
    const value = record[key];
    if (value === null) {
      return null;
    }
    if (typeof value === "number" && Number.isInteger(value)) {
      return value;
    }
  }
  return undefined;
}

function coerceRelayShareLinkRecord(
  value: unknown,
): RelayShareLinkRecord | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }
  const record = value as Record<string, unknown>;

  const id = readApiString(record, ["id"]);
  const targetType = readApiString(record, ["target_type", "targetType"]);
  const targetId = readApiString(record, ["target_id", "targetId"]);
  const permission = readApiString(record, ["permission"]);
  const expiresAt =
    readApiNullableString(record, ["expires_at", "expiresAt"]) ?? null;
  const maxUses =
    readApiNullableInteger(record, ["max_uses", "maxUses"]) ?? null;
  const useCount = readApiNullableInteger(record, ["use_count", "useCount"]);
  const disabled = record.disabled;
  const createdAt = readApiString(record, ["created_at", "createdAt"]);
  const revokedAt =
    readApiNullableString(record, ["revoked_at", "revokedAt"]) ?? null;
  const etag = readApiString(record, ["etag"]);
  const urlOnce = readApiString(record, ["url_once", "urlOnce"]) ?? "";

  if (!id || !targetId || !createdAt || !etag) {
    return null;
  }
  if (targetType !== "workspace" && targetType !== "document") {
    return null;
  }
  if (permission !== "view" && permission !== "edit") {
    return null;
  }
  if (useCount === undefined || useCount === null || useCount < 0) {
    return null;
  }
  if (typeof disabled !== "boolean") {
    return null;
  }
  if (maxUses !== null && maxUses < 1) {
    return null;
  }

  return {
    id,
    targetType,
    targetId,
    permission,
    expiresAt,
    maxUses,
    useCount,
    disabled,
    createdAt,
    revokedAt,
    etag,
    urlOnce,
  };
}

export function buildShareLinkUrl(token: string, origin: string): string {
  const normalizedOrigin = origin.replace(/\/+$/, "");
  return `${normalizedOrigin}/share/${encodeURIComponent(token)}`;
}

export function shareUrlFromCreateShareLinkResponse(
  payload: unknown,
  origin: string,
): string | null {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    return null;
  }

  const envelope = payload as Record<string, unknown>;
  const shareLinkRaw = envelope.share_link ?? envelope.shareLink;
  if (
    !shareLinkRaw ||
    typeof shareLinkRaw !== "object" ||
    Array.isArray(shareLinkRaw)
  ) {
    return null;
  }

  const shareLink = shareLinkRaw as Record<string, unknown>;
  const urlOnce = shareLink.url_once;
  if (typeof urlOnce === "string" && urlOnce.trim().length > 0) {
    return urlOnce;
  }

  const camelUrlOnce = shareLink.urlOnce;
  if (typeof camelUrlOnce === "string" && camelUrlOnce.trim().length > 0) {
    return camelUrlOnce;
  }

  const token = shareLink.token;
  if (typeof token !== "string" || token.trim().length === 0) {
    return null;
  }

  return buildShareLinkUrl(token, origin);
}

export function shareLinkFromCreateShareLinkResponse(
  payload: unknown,
): RelayShareLinkRecord | null {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    return null;
  }
  const envelope = payload as Record<string, unknown>;
  return coerceRelayShareLinkRecord(envelope.share_link ?? envelope.shareLink);
}

export function shareLinksFromListResponse(
  payload: unknown,
): RelayShareLinkRecord[] {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    return [];
  }
  const envelope = payload as Record<string, unknown>;
  const items = envelope.items;
  if (!Array.isArray(items)) {
    return [];
  }
  const parsed = items
    .map((item) => coerceRelayShareLinkRecord(item))
    .filter((item): item is RelayShareLinkRecord => item !== null);

  return parsed.sort((left, right) =>
    right.createdAt.localeCompare(left.createdAt),
  );
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
    option === "24h"
      ? nowMs + 24 * 60 * 60 * 1000
      : nowMs + 7 * 24 * 60 * 60 * 1000;
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

  window.sessionStorage.setItem(
    storageKey(record.token),
    JSON.stringify(record),
  );
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
