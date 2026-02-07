import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";

import {
  leaseBadgeExtension,
  leaseBadgeState,
  LeaseBadgeWidget,
  setLeases,
} from "./lease-badge";

interface BadgeDecoration {
  agentName: string;
  color: string;
  lineNumber: number;
}

function collectBadges(state: EditorState): BadgeDecoration[] {
  const decorations = state.field(leaseBadgeState).decorations;
  const results: BadgeDecoration[] = [];

  decorations.between(0, state.doc.length, (from, _to, value) => {
    const widget = (value.spec as { widget?: LeaseBadgeWidget }).widget;
    if (!widget || !(widget instanceof LeaseBadgeWidget)) {
      return;
    }

    results.push({
      agentName: widget.agentName,
      color: widget.color,
      lineNumber: state.doc.lineAt(from).number,
    });
  });

  results.sort((a, b) => a.lineNumber - b.lineNumber);
  return results;
}

describe("leaseBadgeExtension", () => {
  const DOC = "# Top\nalpha\n## Sub\nbeta";

  it("renders lease badges at heading lines", () => {
    const state = EditorState.create({
      doc: DOC,
      extensions: [leaseBadgeExtension()],
    }).update({
      effects: [
        setLeases.of([
          { agentName: "Alice", color: "#e06c75", headingLine: 1 },
          { agentName: "Bob", color: "#61afef", headingLine: 3 },
        ]),
      ],
    }).state;

    expect(collectBadges(state)).toEqual([
      { agentName: "Alice", color: "#e06c75", lineNumber: 1 },
      { agentName: "Bob", color: "#61afef", lineNumber: 3 },
    ]);
  });

  it("skips leases with out-of-range line numbers", () => {
    const state = EditorState.create({
      doc: DOC,
      extensions: [leaseBadgeExtension()],
    }).update({
      effects: [
        setLeases.of([
          { agentName: "Valid", color: "#98c379", headingLine: 1 },
          { agentName: "TooHigh", color: "#c678dd", headingLine: 99 },
          { agentName: "Zero", color: "#d19a66", headingLine: 0 },
          { agentName: "Negative", color: "#be5046", headingLine: -1 },
        ]),
      ],
    }).state;

    expect(collectBadges(state)).toEqual([
      { agentName: "Valid", color: "#98c379", lineNumber: 1 },
    ]);
  });

  it("replaces previous leases on new effect", () => {
    let state = EditorState.create({
      doc: DOC,
      extensions: [leaseBadgeExtension()],
    });

    state = state.update({
      effects: [
        setLeases.of([
          { agentName: "First", color: "#e06c75", headingLine: 1 },
        ]),
      ],
    }).state;
    expect(collectBadges(state)).toHaveLength(1);

    state = state.update({
      effects: [
        setLeases.of([
          { agentName: "Second", color: "#61afef", headingLine: 3 },
        ]),
      ],
    }).state;

    const badges = collectBadges(state);
    expect(badges).toHaveLength(1);
    expect(badges[0].agentName).toBe("Second");
  });

  it("clears badges when empty array dispatched", () => {
    let state = EditorState.create({
      doc: DOC,
      extensions: [leaseBadgeExtension()],
    });

    state = state.update({
      effects: [
        setLeases.of([
          { agentName: "Agent", color: "#e06c75", headingLine: 1 },
        ]),
      ],
    }).state;
    expect(collectBadges(state)).toHaveLength(1);

    state = state.update({
      effects: [setLeases.of([])],
    }).state;
    expect(collectBadges(state)).toHaveLength(0);
  });

  it("supports multiple badges on different lines", () => {
    const doc = "# H1\ntext\n## H2\ntext\n### H3";
    const state = EditorState.create({
      doc,
      extensions: [leaseBadgeExtension()],
    }).update({
      effects: [
        setLeases.of([
          { agentName: "A", color: "#e06c75", headingLine: 1 },
          { agentName: "B", color: "#61afef", headingLine: 3 },
          { agentName: "C", color: "#98c379", headingLine: 5 },
        ]),
      ],
    }).state;

    expect(collectBadges(state)).toHaveLength(3);
  });
});

describe("LeaseBadgeWidget", () => {
  it("eq returns true for matching widgets", () => {
    const a = new LeaseBadgeWidget("Agent", "#e06c75", undefined);
    const b = new LeaseBadgeWidget("Agent", "#e06c75", undefined);
    expect(a.eq(b)).toBe(true);
  });

  it("eq returns false when name differs", () => {
    const a = new LeaseBadgeWidget("Alice", "#e06c75", undefined);
    const b = new LeaseBadgeWidget("Bob", "#e06c75", undefined);
    expect(a.eq(b)).toBe(false);
  });

  it("eq returns false when color differs", () => {
    const a = new LeaseBadgeWidget("Agent", "#e06c75", undefined);
    const b = new LeaseBadgeWidget("Agent", "#61afef", undefined);
    expect(a.eq(b)).toBe(false);
  });

  it("eq returns false when expiresAt differs", () => {
    const a = new LeaseBadgeWidget("Agent", "#e06c75", 1000);
    const b = new LeaseBadgeWidget("Agent", "#e06c75", 2000);
    expect(a.eq(b)).toBe(false);
  });
});
