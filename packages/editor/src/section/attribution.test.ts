import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";

import {
  attributionExtension,
  attributionState,
  AttributionBadgeWidget,
  setAttributions,
  type EditorType,
  type SectionContributor,
} from "./attribution";

interface BadgeInfo {
  name: string;
  editorType: EditorType;
  lineNumber: number;
}

function collectBadges(state: EditorState): BadgeInfo[] {
  const decorations = state.field(attributionState).decorations;
  const results: BadgeInfo[] = [];

  decorations.between(0, state.doc.length, (from, _to, value) => {
    const widget = (value.spec as { widget?: AttributionBadgeWidget }).widget;
    if (!widget || !(widget instanceof AttributionBadgeWidget)) {
      return;
    }

    results.push({
      name: widget.name,
      editorType: widget.editorType,
      lineNumber: state.doc.lineAt(from).number,
    });
  });

  results.sort((a, b) => a.lineNumber - b.lineNumber);
  return results;
}

describe("attributionExtension", () => {
  const DOC = "# Top\nalpha\n## Sub\nbeta";

  it("renders attribution badges at heading lines", () => {
    const state = EditorState.create({
      doc: DOC,
      extensions: [attributionExtension()],
    }).update({
      effects: [
        setAttributions.of([
          {
            headingLine: 1,
            lastEditedBy: "Alice",
            lastEditorType: "human",
            color: "#e06c75",
            contributors: [],
          },
          {
            headingLine: 3,
            lastEditedBy: "cursor-agent",
            lastEditorType: "agent",
            color: "#61afef",
            contributors: [],
          },
        ]),
      ],
    }).state;

    expect(collectBadges(state)).toEqual([
      { name: "Alice", editorType: "human", lineNumber: 1 },
      { name: "cursor-agent", editorType: "agent", lineNumber: 3 },
    ]);
  });

  it("skips attributions with out-of-range line numbers", () => {
    const state = EditorState.create({
      doc: DOC,
      extensions: [attributionExtension()],
    }).update({
      effects: [
        setAttributions.of([
          {
            headingLine: 1,
            lastEditedBy: "Valid",
            lastEditorType: "human",
            color: "#98c379",
            contributors: [],
          },
          {
            headingLine: 99,
            lastEditedBy: "TooHigh",
            lastEditorType: "agent",
            color: "#c678dd",
            contributors: [],
          },
          {
            headingLine: 0,
            lastEditedBy: "Zero",
            lastEditorType: "human",
            color: "#d19a66",
            contributors: [],
          },
        ]),
      ],
    }).state;

    expect(collectBadges(state)).toEqual([
      { name: "Valid", editorType: "human", lineNumber: 1 },
    ]);
  });

  it("replaces previous attributions on new effect", () => {
    let state = EditorState.create({
      doc: DOC,
      extensions: [attributionExtension()],
    });

    state = state.update({
      effects: [
        setAttributions.of([
          {
            headingLine: 1,
            lastEditedBy: "First",
            lastEditorType: "human",
            color: "#e06c75",
            contributors: [],
          },
        ]),
      ],
    }).state;
    expect(collectBadges(state)).toHaveLength(1);

    state = state.update({
      effects: [
        setAttributions.of([
          {
            headingLine: 3,
            lastEditedBy: "Second",
            lastEditorType: "agent",
            color: "#61afef",
            contributors: [],
          },
        ]),
      ],
    }).state;

    const badges = collectBadges(state);
    expect(badges).toHaveLength(1);
    expect(badges[0].name).toBe("Second");
    expect(badges[0].editorType).toBe("agent");
  });

  it("clears badges when empty array dispatched", () => {
    let state = EditorState.create({
      doc: DOC,
      extensions: [attributionExtension()],
    });

    state = state.update({
      effects: [
        setAttributions.of([
          {
            headingLine: 1,
            lastEditedBy: "Agent",
            lastEditorType: "agent",
            color: "#e06c75",
            contributors: [],
          },
        ]),
      ],
    }).state;
    expect(collectBadges(state)).toHaveLength(1);

    state = state.update({
      effects: [setAttributions.of([])],
    }).state;
    expect(collectBadges(state)).toHaveLength(0);
  });

  it("supports multiple badges on different lines", () => {
    const doc = "# H1\ntext\n## H2\ntext\n### H3";
    const state = EditorState.create({
      doc,
      extensions: [attributionExtension()],
    }).update({
      effects: [
        setAttributions.of([
          {
            headingLine: 1,
            lastEditedBy: "A",
            lastEditorType: "human",
            color: "#e06c75",
            contributors: [],
          },
          {
            headingLine: 3,
            lastEditedBy: "B",
            lastEditorType: "agent",
            color: "#61afef",
            contributors: [],
          },
          {
            headingLine: 5,
            lastEditedBy: "C",
            lastEditorType: "human",
            color: "#98c379",
            contributors: [],
          },
        ]),
      ],
    }).state;

    expect(collectBadges(state)).toHaveLength(3);
  });

  it("preserves contributor data in state", () => {
    const contributors: SectionContributor[] = [
      { name: "Alice", type: "human", charCount: 500 },
      { name: "cursor-agent", type: "agent", charCount: 300 },
    ];

    const state = EditorState.create({
      doc: DOC,
      extensions: [attributionExtension()],
    }).update({
      effects: [
        setAttributions.of([
          {
            headingLine: 1,
            lastEditedBy: "Alice",
            lastEditorType: "human",
            color: "#e06c75",
            contributors,
          },
        ]),
      ],
    }).state;

    const stored = state.field(attributionState).attributions;
    expect(stored).toHaveLength(1);
    expect(stored[0].contributors).toEqual(contributors);
  });
});

describe("AttributionBadgeWidget", () => {
  it("eq returns true for matching widgets", () => {
    const contributors: SectionContributor[] = [
      { name: "Alice", type: "human", charCount: 100 },
    ];
    const a = new AttributionBadgeWidget(
      "Alice",
      "human",
      "#e06c75",
      undefined,
      contributors,
    );
    const b = new AttributionBadgeWidget(
      "Alice",
      "human",
      "#e06c75",
      undefined,
      contributors,
    );
    expect(a.eq(b)).toBe(true);
  });

  it("eq returns false when name differs", () => {
    const a = new AttributionBadgeWidget("Alice", "human", "#e06c75", undefined, []);
    const b = new AttributionBadgeWidget("Bob", "human", "#e06c75", undefined, []);
    expect(a.eq(b)).toBe(false);
  });

  it("eq returns false when type differs", () => {
    const a = new AttributionBadgeWidget("Agent", "human", "#e06c75", undefined, []);
    const b = new AttributionBadgeWidget("Agent", "agent", "#e06c75", undefined, []);
    expect(a.eq(b)).toBe(false);
  });

  it("eq returns false when contributors differ", () => {
    const a = new AttributionBadgeWidget("Alice", "human", "#e06c75", undefined, [
      { name: "Alice", type: "human", charCount: 100 },
    ]);
    const b = new AttributionBadgeWidget("Alice", "human", "#e06c75", undefined, [
      { name: "Alice", type: "human", charCount: 200 },
    ]);
    expect(a.eq(b)).toBe(false);
  });

  it("eq returns false when contributor count differs", () => {
    const a = new AttributionBadgeWidget("Alice", "human", "#e06c75", undefined, [
      { name: "Alice", type: "human", charCount: 100 },
    ]);
    const b = new AttributionBadgeWidget("Alice", "human", "#e06c75", undefined, []);
    expect(a.eq(b)).toBe(false);
  });
});
