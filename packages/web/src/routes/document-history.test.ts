import { describe, expect, it } from "vitest";
import {
  buildAuthorshipSegments,
  createTimelineSnapshotEntry,
  deriveTimelineSnapshotEntry,
  timelineAuthorFromPeer,
  type TimelineAuthor,
} from "./document";

function author(id: string, name: string, color: string): TimelineAuthor {
  return {
    color,
    id,
    name,
    type: "human",
  };
}

describe("document history attribution helpers", () => {
  it("creates a snapshot entry with per-character attribution", () => {
    const alice = author("alice", "Alice", "#3366cc");
    const entry = createTimelineSnapshotEntry("abc", alice);

    expect(entry.content).toBe("abc");
    expect(entry.attribution).toEqual([alice, alice, alice]);
  });

  it("derives next snapshot and preserves unchanged prefix/suffix attribution", () => {
    const alice = author("alice", "Alice", "#3366cc");
    const bob = author("bob", "Bob", "#ff6600");
    const initial = createTimelineSnapshotEntry("abcXYZ", alice);

    const next = deriveTimelineSnapshotEntry(initial, "abc12XYZ", bob);
    const segments = buildAuthorshipSegments(next);

    expect(next.content).toBe("abc12XYZ");
    expect(next.attribution).toHaveLength("abc12XYZ".length);
    expect(segments).toEqual([
      { author: alice, text: "abc" },
      { author: bob, text: "12" },
      { author: alice, text: "XYZ" },
    ]);
  });

  it("assigns replacement ranges to the latest author", () => {
    const alice = author("alice", "Alice", "#3366cc");
    const bob = author("bob", "Bob", "#ff6600");
    const initial = createTimelineSnapshotEntry("hello world", alice);

    const next = deriveTimelineSnapshotEntry(initial, "hello there", bob);
    const segments = buildAuthorshipSegments(next);

    expect(segments).toEqual([
      { author: alice, text: "hello " },
      { author: bob, text: "there" },
    ]);
  });

  it("converts peer presence to deterministic timeline author metadata", () => {
    const remote = timelineAuthorFromPeer({ name: "Claude Agent", type: "agent" });

    expect(remote).toEqual({
      color: expect.any(String),
      id: "peer:claude-agent",
      name: "Claude Agent",
      type: "agent",
    });
  });
});
