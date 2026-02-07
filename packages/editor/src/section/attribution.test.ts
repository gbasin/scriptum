import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";

import {
  AttributionBadgeWidget,
  attributionExtension,
  attributionState,
  buildAttributionBadgeText,
  formatRelativeTimestamp,
  type EditorType,
  type SectionAttribution,
  type SectionContributor,
  setAttributions,
} from "./attribution";

const DEFAULT_LAST_EDITED_AT = "2026-02-07T12:00:00Z";

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

function createAttribution(
  overrides: Pick<
    SectionAttribution,
    "headingLine" | "lastEditedBy" | "lastEditorType" | "color"
  > &
    Partial<
      Omit<
        SectionAttribution,
        "headingLine" | "lastEditedBy" | "lastEditorType" | "color"
      >
    >,
): SectionAttribution {
  return {
    headingLine: overrides.headingLine,
    authorId:
      overrides.authorId ??
      overrides.lastEditedBy.toLowerCase().replace(/[^a-z0-9]+/g, "-"),
    lastEditedBy: overrides.lastEditedBy,
    lastEditorType: overrides.lastEditorType,
    color: overrides.color,
    lastEditedAt: overrides.lastEditedAt ?? DEFAULT_LAST_EDITED_AT,
    contributors: overrides.contributors ?? [],
  };
}

describe("attributionExtension", () => {
  const DOC = "# Top\nalpha\n## Sub\nbeta";

  it("renders attribution badges at heading lines", () => {
    const state = EditorState.create({
      doc: DOC,
      extensions: [attributionExtension()],
    })
      .update({
        effects: [
          setAttributions.of([
            createAttribution({
              headingLine: 1,
              lastEditedBy: "Alice",
              lastEditorType: "human",
              color: "#e06c75",
            }),
            createAttribution({
              headingLine: 3,
              lastEditedBy: "cursor-agent",
              lastEditorType: "agent",
              color: "#61afef",
            }),
          ]),
        ],
      })
      .state;

    expect(collectBadges(state)).toEqual([
      { name: "Alice", editorType: "human", lineNumber: 1 },
      { name: "cursor-agent", editorType: "agent", lineNumber: 3 },
    ]);
  });

  it("skips attributions with out-of-range line numbers", () => {
    const state = EditorState.create({
      doc: DOC,
      extensions: [attributionExtension()],
    })
      .update({
        effects: [
          setAttributions.of([
            createAttribution({
              headingLine: 1,
              lastEditedBy: "Valid",
              lastEditorType: "human",
              color: "#98c379",
            }),
            createAttribution({
              headingLine: 99,
              lastEditedBy: "TooHigh",
              lastEditorType: "agent",
              color: "#c678dd",
            }),
            createAttribution({
              headingLine: 0,
              lastEditedBy: "Zero",
              lastEditorType: "human",
              color: "#d19a66",
            }),
          ]),
        ],
      })
      .state;

    expect(collectBadges(state)).toEqual([
      { name: "Valid", editorType: "human", lineNumber: 1 },
    ]);
  });

  it("replaces previous attributions on new effect", () => {
    let state = EditorState.create({
      doc: DOC,
      extensions: [attributionExtension()],
    });

    state = state
      .update({
        effects: [
          setAttributions.of([
            createAttribution({
              headingLine: 1,
              lastEditedBy: "First",
              lastEditorType: "human",
              color: "#e06c75",
            }),
          ]),
        ],
      })
      .state;
    expect(collectBadges(state)).toHaveLength(1);

    state = state
      .update({
        effects: [
          setAttributions.of([
            createAttribution({
              headingLine: 3,
              lastEditedBy: "Second",
              lastEditorType: "agent",
              color: "#61afef",
            }),
          ]),
        ],
      })
      .state;

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

    state = state
      .update({
        effects: [
          setAttributions.of([
            createAttribution({
              headingLine: 1,
              lastEditedBy: "Agent",
              lastEditorType: "agent",
              color: "#e06c75",
            }),
          ]),
        ],
      })
      .state;
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
    })
      .update({
        effects: [
          setAttributions.of([
            createAttribution({
              headingLine: 1,
              lastEditedBy: "A",
              lastEditorType: "human",
              color: "#e06c75",
            }),
            createAttribution({
              headingLine: 3,
              lastEditedBy: "B",
              lastEditorType: "agent",
              color: "#61afef",
            }),
            createAttribution({
              headingLine: 5,
              lastEditedBy: "C",
              lastEditorType: "human",
              color: "#98c379",
            }),
          ]),
        ],
      })
      .state;

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
    })
      .update({
        effects: [
          setAttributions.of([
            createAttribution({
              headingLine: 1,
              lastEditedBy: "Alice",
              lastEditorType: "human",
              color: "#e06c75",
              contributors,
            }),
          ]),
        ],
      })
      .state;

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
      "alice",
      "human",
      "#e06c75",
      DEFAULT_LAST_EDITED_AT,
      contributors,
    );
    const b = new AttributionBadgeWidget(
      "Alice",
      "alice",
      "human",
      "#e06c75",
      DEFAULT_LAST_EDITED_AT,
      contributors,
    );
    expect(a.eq(b)).toBe(true);
  });

  it("eq returns false when author id differs", () => {
    const a = new AttributionBadgeWidget(
      "Alice",
      "alice",
      "human",
      "#e06c75",
      DEFAULT_LAST_EDITED_AT,
      [],
    );
    const b = new AttributionBadgeWidget(
      "Alice",
      "alice-2",
      "human",
      "#e06c75",
      DEFAULT_LAST_EDITED_AT,
      [],
    );
    expect(a.eq(b)).toBe(false);
  });

  it("eq returns false when type differs", () => {
    const a = new AttributionBadgeWidget(
      "Agent",
      "agent",
      "human",
      "#e06c75",
      DEFAULT_LAST_EDITED_AT,
      [],
    );
    const b = new AttributionBadgeWidget(
      "Agent",
      "agent",
      "agent",
      "#e06c75",
      DEFAULT_LAST_EDITED_AT,
      [],
    );
    expect(a.eq(b)).toBe(false);
  });

  it("eq returns false when contributors differ", () => {
    const a = new AttributionBadgeWidget(
      "Alice",
      "alice",
      "human",
      "#e06c75",
      DEFAULT_LAST_EDITED_AT,
      [{ name: "Alice", type: "human", charCount: 100 }],
    );
    const b = new AttributionBadgeWidget(
      "Alice",
      "alice",
      "human",
      "#e06c75",
      DEFAULT_LAST_EDITED_AT,
      [{ name: "Alice", type: "human", charCount: 200 }],
    );
    expect(a.eq(b)).toBe(false);
  });

  it("renders type icon and relative edit time text", () => {
    const nowMs = Date.parse("2026-02-07T12:00:00Z");
    expect(
      buildAttributionBadgeText("Alice", "human", "2026-02-07T11:55:00Z", nowMs),
    ).toBe("👤 Alice · 5m ago");
    expect(
      buildAttributionBadgeText(
        "Cursor",
        "agent",
        "2026-02-07T11:59:00Z",
        nowMs,
      ),
    ).toBe("⚙ Cursor · 1m ago");
  });
});

describe("formatRelativeTimestamp", () => {
  const nowMs = Date.parse("2026-02-07T12:00:00Z");

  it("formats past timestamps", () => {
    expect(formatRelativeTimestamp("2026-02-07T11:55:00Z", nowMs)).toBe(
      "5m ago",
    );
    expect(formatRelativeTimestamp("2026-02-07T10:00:00Z", nowMs)).toBe(
      "2h ago",
    );
  });

  it("formats future timestamps", () => {
    expect(formatRelativeTimestamp("2026-02-07T12:03:00Z", nowMs)).toBe(
      "in 3m",
    );
  });

  it("falls back to input for invalid timestamps", () => {
    expect(formatRelativeTimestamp("not-a-date", nowMs)).toBe("not-a-date");
  });
});
