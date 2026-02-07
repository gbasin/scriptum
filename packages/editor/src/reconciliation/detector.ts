export const RECONCILIATION_WINDOW_MS = 30_000;
export const RECONCILIATION_THRESHOLD_RATIO = 0.5;

export interface SectionEditEvent {
  readonly sectionId: string;
  readonly authorId: string;
  readonly timestampMs: number;
  readonly changedChars: number;
  readonly sectionLength: number;
}

export interface SectionEditHistoryEntry {
  readonly authorId: string;
  readonly timestampMs: number;
  readonly changedChars: number;
  readonly sectionLength: number;
}

export interface ReconciliationWindowStats {
  readonly sectionId: string;
  readonly editCount: number;
  readonly distinctAuthorCount: number;
  readonly totalChangedChars: number;
  readonly sectionLength: number;
  readonly changeRatio: number;
  readonly oldestEditTimestampMs: number | null;
  readonly newestEditTimestampMs: number | null;
}

export interface ReconciliationTrigger {
  readonly sectionId: string;
  readonly triggeredAtMs: number;
  readonly stats: ReconciliationWindowStats;
}

export interface ReconciliationDetectorOptions {
  readonly windowMs?: number;
  readonly thresholdRatio?: number;
}

export function shouldTriggerReconciliation(
  stats: ReconciliationWindowStats,
  thresholdRatio = RECONCILIATION_THRESHOLD_RATIO,
): boolean {
  return (
    stats.sectionLength > 0 &&
    stats.distinctAuthorCount >= 2 &&
    stats.changeRatio > thresholdRatio
  );
}

export class ReconciliationDetector {
  readonly windowMs: number;
  readonly thresholdRatio: number;

  private readonly entriesBySection = new Map<
    string,
    SectionEditHistoryEntry[]
  >();

  constructor(options: ReconciliationDetectorOptions = {}) {
    this.windowMs = normalizeWindowMs(options.windowMs);
    this.thresholdRatio = normalizeThresholdRatio(options.thresholdRatio);
  }

  recordEdit(edit: SectionEditEvent): ReconciliationTrigger | null {
    const sectionId = normalizeNonEmpty(edit.sectionId, "sectionId");
    const normalized: SectionEditHistoryEntry = {
      authorId: normalizeNonEmpty(edit.authorId, "authorId"),
      timestampMs: normalizeTimestamp(edit.timestampMs),
      changedChars: normalizeCount(edit.changedChars, "changedChars"),
      sectionLength: normalizeCount(edit.sectionLength, "sectionLength"),
    };

    const existing = this.entriesBySection.get(sectionId) ?? [];
    const merged = [...existing, normalized].sort(
      (left, right) => left.timestampMs - right.timestampMs,
    );

    const latestTimestampMs = merged[merged.length - 1].timestampMs;
    const persistedMinTimestampMs = latestTimestampMs - this.windowMs;
    const persisted = merged.filter(
      (entry) => entry.timestampMs >= persistedMinTimestampMs,
    );

    if (persisted.length === 0) {
      this.entriesBySection.delete(sectionId);
    } else {
      this.entriesBySection.set(sectionId, persisted);
    }

    const evalStartMs = normalized.timestampMs - this.windowMs;
    const relevantAfter = persisted.filter(
      (entry) =>
        entry.timestampMs >= evalStartMs &&
        entry.timestampMs <= normalized.timestampMs,
    );
    const relevantBefore = relevantAfter.filter(
      (entry) => entry !== normalized,
    );

    const beforeStats = buildStats(sectionId, relevantBefore);
    const afterStats = buildStats(sectionId, relevantAfter);
    const crossedThreshold =
      !shouldTriggerReconciliation(beforeStats, this.thresholdRatio) &&
      shouldTriggerReconciliation(afterStats, this.thresholdRatio);

    if (!crossedThreshold) {
      return null;
    }

    return {
      sectionId,
      triggeredAtMs: normalized.timestampMs,
      stats: afterStats,
    };
  }

  getSectionHistory(
    sectionId: string,
    nowMs = Date.now(),
  ): readonly SectionEditHistoryEntry[] {
    const normalizedSectionId = normalizeNonEmpty(sectionId, "sectionId");
    const pruned = this.pruneSection(
      normalizedSectionId,
      normalizeTimestamp(nowMs),
    );
    return [...pruned];
  }

  getSectionStats(
    sectionId: string,
    nowMs = Date.now(),
  ): ReconciliationWindowStats {
    const normalizedSectionId = normalizeNonEmpty(sectionId, "sectionId");
    const pruned = this.pruneSection(
      normalizedSectionId,
      normalizeTimestamp(nowMs),
    );
    return buildStats(normalizedSectionId, pruned);
  }

  clearSection(sectionId: string): void {
    this.entriesBySection.delete(normalizeNonEmpty(sectionId, "sectionId"));
  }

  clear(): void {
    this.entriesBySection.clear();
  }

  private pruneSection(
    sectionId: string,
    nowMs: number,
  ): readonly SectionEditHistoryEntry[] {
    const entries = this.entriesBySection.get(sectionId);
    if (!entries || entries.length === 0) {
      return [];
    }

    const minTimestampMs = nowMs - this.windowMs;
    const pruned = entries.filter(
      (entry) => entry.timestampMs >= minTimestampMs,
    );

    if (pruned.length === 0) {
      this.entriesBySection.delete(sectionId);
      return [];
    }

    this.entriesBySection.set(sectionId, pruned);
    return pruned;
  }
}

function buildStats(
  sectionId: string,
  entries: readonly SectionEditHistoryEntry[],
): ReconciliationWindowStats {
  if (entries.length === 0) {
    return {
      sectionId,
      editCount: 0,
      distinctAuthorCount: 0,
      totalChangedChars: 0,
      sectionLength: 0,
      changeRatio: 0,
      oldestEditTimestampMs: null,
      newestEditTimestampMs: null,
    };
  }

  let totalChangedChars = 0;
  let sectionLength = 0;
  let oldestEditTimestampMs = Number.POSITIVE_INFINITY;
  let newestEditTimestampMs = Number.NEGATIVE_INFINITY;
  const authors = new Set<string>();

  for (const entry of entries) {
    totalChangedChars += entry.changedChars;
    sectionLength = Math.max(sectionLength, entry.sectionLength);
    oldestEditTimestampMs = Math.min(oldestEditTimestampMs, entry.timestampMs);
    newestEditTimestampMs = Math.max(newestEditTimestampMs, entry.timestampMs);
    authors.add(entry.authorId);
  }

  return {
    sectionId,
    editCount: entries.length,
    distinctAuthorCount: authors.size,
    totalChangedChars,
    sectionLength,
    changeRatio: sectionLength > 0 ? totalChangedChars / sectionLength : 0,
    oldestEditTimestampMs,
    newestEditTimestampMs,
  };
}

function normalizeNonEmpty(value: string, field: string): string {
  const normalized = value.trim();
  if (normalized.length === 0) {
    throw new Error(`${field} must not be empty`);
  }
  return normalized;
}

function normalizeTimestamp(value: number): number {
  if (!Number.isFinite(value)) {
    throw new Error("timestampMs must be a finite number");
  }
  return value;
}

function normalizeCount(value: number, field: string): number {
  if (!Number.isFinite(value)) {
    throw new Error(`${field} must be a finite number`);
  }
  return Math.max(0, Math.floor(value));
}

function normalizeWindowMs(value: number | undefined): number {
  if (value === undefined) {
    return RECONCILIATION_WINDOW_MS;
  }
  if (!Number.isFinite(value) || value <= 0) {
    throw new Error("windowMs must be a positive number");
  }
  return Math.floor(value);
}

function normalizeThresholdRatio(value: number | undefined): number {
  if (value === undefined) {
    return RECONCILIATION_THRESHOLD_RATIO;
  }
  if (!Number.isFinite(value) || value < 0) {
    throw new Error("thresholdRatio must be a non-negative number");
  }
  return value;
}
