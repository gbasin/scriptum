import { describe, expect, it } from "vitest";

import {
  ReconciliationDetector,
  RECONCILIATION_THRESHOLD_RATIO,
  RECONCILIATION_WINDOW_MS,
  shouldTriggerReconciliation,
} from "./detector";

describe("ReconciliationDetector", () => {
  it("triggers when >50% of a section changes by 2+ authors within 30 seconds", () => {
    const detector = new ReconciliationDetector();

    const first = detector.recordEdit({
      sectionId: "sec-intro",
      authorId: "alice",
      timestampMs: 1_000,
      changedChars: 40,
      sectionLength: 100,
    });
    expect(first).toBeNull();

    const second = detector.recordEdit({
      sectionId: "sec-intro",
      authorId: "bob",
      timestampMs: 10_000,
      changedChars: 20,
      sectionLength: 100,
    });

    expect(second).not.toBeNull();
    expect(second?.sectionId).toBe("sec-intro");
    expect(second?.stats.distinctAuthorCount).toBe(2);
    expect(second?.stats.changeRatio).toBeCloseTo(0.6);
  });

  it("does not trigger for a single author even when change ratio exceeds threshold", () => {
    const detector = new ReconciliationDetector();

    detector.recordEdit({
      sectionId: "sec-single-author",
      authorId: "alice",
      timestampMs: 5_000,
      changedChars: 30,
      sectionLength: 40,
    });
    const result = detector.recordEdit({
      sectionId: "sec-single-author",
      authorId: "alice",
      timestampMs: 8_000,
      changedChars: 10,
      sectionLength: 40,
    });

    expect(result).toBeNull();
  });

  it("uses a strict greater-than threshold (50% does not trigger)", () => {
    const detector = new ReconciliationDetector();

    detector.recordEdit({
      sectionId: "sec-boundary",
      authorId: "alice",
      timestampMs: 1_000,
      changedChars: 25,
      sectionLength: 100,
    });

    const equalThreshold = detector.recordEdit({
      sectionId: "sec-boundary",
      authorId: "bob",
      timestampMs: 2_000,
      changedChars: 25,
      sectionLength: 100,
    });
    expect(equalThreshold).toBeNull();

    const aboveThreshold = detector.recordEdit({
      sectionId: "sec-boundary",
      authorId: "bob",
      timestampMs: 3_000,
      changedChars: 1,
      sectionLength: 100,
    });
    expect(aboveThreshold).not.toBeNull();
    expect(aboveThreshold?.stats.changeRatio).toBeCloseTo(0.51);
  });

  it("does not trigger when edits are outside the 30-second window", () => {
    const detector = new ReconciliationDetector();

    detector.recordEdit({
      sectionId: "sec-window",
      authorId: "alice",
      timestampMs: 0,
      changedChars: 40,
      sectionLength: 100,
    });
    const result = detector.recordEdit({
      sectionId: "sec-window",
      authorId: "bob",
      timestampMs: RECONCILIATION_WINDOW_MS + 1_000,
      changedChars: 20,
      sectionLength: 100,
    });

    expect(result).toBeNull();
  });

  it("tracks per-section edit history with author and timestamp", () => {
    const detector = new ReconciliationDetector();

    detector.recordEdit({
      sectionId: "sec-history",
      authorId: "alice",
      timestampMs: 2_000,
      changedChars: 8,
      sectionLength: 120,
    });
    detector.recordEdit({
      sectionId: "sec-history",
      authorId: "bob",
      timestampMs: 4_000,
      changedChars: 16,
      sectionLength: 120,
    });

    const history = detector.getSectionHistory("sec-history", 5_000);
    expect(history).toEqual([
      {
        authorId: "alice",
        timestampMs: 2_000,
        changedChars: 8,
        sectionLength: 120,
      },
      {
        authorId: "bob",
        timestampMs: 4_000,
        changedChars: 16,
        sectionLength: 120,
      },
    ]);
  });

  it("prunes stale entries and can trigger again in a later window", () => {
    const detector = new ReconciliationDetector();

    detector.recordEdit({
      sectionId: "sec-repeat",
      authorId: "alice",
      timestampMs: 0,
      changedChars: 40,
      sectionLength: 100,
    });
    const firstTrigger = detector.recordEdit({
      sectionId: "sec-repeat",
      authorId: "bob",
      timestampMs: 1_000,
      changedChars: 20,
      sectionLength: 100,
    });
    expect(firstTrigger).not.toBeNull();

    const noRetrigger = detector.recordEdit({
      sectionId: "sec-repeat",
      authorId: "carol",
      timestampMs: 2_000,
      changedChars: 5,
      sectionLength: 100,
    });
    expect(noRetrigger).toBeNull();

    const secondWindowTrigger = detector.recordEdit({
      sectionId: "sec-repeat",
      authorId: "alice",
      timestampMs: RECONCILIATION_WINDOW_MS + 5_000,
      changedChars: 40,
      sectionLength: 100,
    });
    expect(secondWindowTrigger).toBeNull();

    const secondTrigger = detector.recordEdit({
      sectionId: "sec-repeat",
      authorId: "bob",
      timestampMs: RECONCILIATION_WINDOW_MS + 8_000,
      changedChars: 20,
      sectionLength: 100,
    });
    expect(secondTrigger).not.toBeNull();
  });
});

describe("shouldTriggerReconciliation", () => {
  it("respects the default threshold and author count requirements", () => {
    const trigger = shouldTriggerReconciliation({
      sectionId: "sec",
      editCount: 2,
      distinctAuthorCount: 2,
      totalChangedChars: 60,
      sectionLength: 100,
      changeRatio: 0.6,
      oldestEditTimestampMs: 0,
      newestEditTimestampMs: 1_000,
    });
    expect(trigger).toBe(true);

    const noTrigger = shouldTriggerReconciliation({
      sectionId: "sec",
      editCount: 2,
      distinctAuthorCount: 1,
      totalChangedChars: 90,
      sectionLength: 100,
      changeRatio: 0.9,
      oldestEditTimestampMs: 0,
      newestEditTimestampMs: 1_000,
    });
    expect(noTrigger).toBe(false);
  });

  it("accepts custom thresholds", () => {
    const trigger = shouldTriggerReconciliation(
      {
        sectionId: "sec",
        editCount: 2,
        distinctAuthorCount: 2,
        totalChangedChars: 45,
        sectionLength: 100,
        changeRatio: 0.45,
        oldestEditTimestampMs: 0,
        newestEditTimestampMs: 1_000,
      },
      RECONCILIATION_THRESHOLD_RATIO - 0.1,
    );

    expect(trigger).toBe(true);
  });
});
