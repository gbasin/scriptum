import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import {
  activeLines,
  getMarkdownNodes,
  headingPreviewDecorations,
  isLineActive,
  livePreview,
} from "./extension.js";

describe("livePreview", () => {
  it("loads extension and initializes active-line tracking", () => {
    const state = EditorState.create({
      doc: "# Title\n\nBody",
      extensions: [livePreview()],
    });

    expect(state.field(activeLines)).toBe(1);
    expect(isLineActive(state, 1)).toBe(true);
    expect(isLineActive(state, 2)).toBe(false);
  });

  it("parses markdown into a tree analysis", () => {
    const source = "# Heading\n\nBody text";
    const analysis = getMarkdownNodes(source);

    expect(analysis.rootNode).toBe("Document");
    expect(analysis.length).toBe(source.length);
    expect(analysis.topLevelNodeCount).toBeGreaterThan(0);
  });

  it("renders every heading level on unfocused lines", () => {
    const source = [
      "# H1",
      "## H2",
      "### H3",
      "#### H4",
      "##### H5",
      "###### H6",
      "body",
    ].join("\n");

    const state = EditorState.create({
      doc: source,
      selection: { anchor: source.length },
      extensions: [livePreview()],
    });

    const classes = collectHeadingClasses(state);
    for (let level = 1; level <= 6; level += 1) {
      expect(classes.has(`cm-livePreview-heading-h${level}`)).toBe(true);
    }
  });

  it("keeps active heading line raw markdown", () => {
    const state = EditorState.create({
      doc: "# Active\n## Inactive",
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    expect(hasDecorationOnLine(state, 1)).toBe(false);
    expect(hasDecorationOnLine(state, 2)).toBe(true);
  });

  it("updates heading decoration level after markdown heading level changes", () => {
    let state = EditorState.create({
      doc: "## Heading\nbody",
      selection: { anchor: "## Heading\n".length },
      extensions: [livePreview()],
    });

    expect(collectHeadingClasses(state).has("cm-livePreview-heading-h2")).toBe(true);

    const firstLine = state.doc.line(1);
    state = state.update({
      changes: {
        from: firstLine.from,
        to: firstLine.to,
        insert: "### Heading",
      },
    }).state;

    const classes = collectHeadingClasses(state);
    expect(classes.has("cm-livePreview-heading-h3")).toBe(true);
    expect(classes.has("cm-livePreview-heading-h2")).toBe(false);
  });
});

function collectHeadingClasses(state: EditorState): Set<string> {
  const classes = new Set<string>();
  const decorations = state.field(headingPreviewDecorations);

  decorations.between(0, state.doc.length, (_from, _to, value) => {
    const spec = (value as {
      spec?: { class?: unknown; attributes?: { class?: unknown } };
    }).spec;
    const className =
      typeof spec?.class === "string"
        ? spec.class
        : typeof spec?.attributes?.class === "string"
          ? spec.attributes.class
          : null;

    if (!className) {
      return;
    }

    for (const token of className.split(/\s+/)) {
      if (token.startsWith("cm-livePreview-heading-h")) {
        classes.add(token);
      }
    }
  });

  return classes;
}

function hasDecorationOnLine(state: EditorState, lineNumber: number): boolean {
  const line = state.doc.line(lineNumber);
  const decorations = state.field(headingPreviewDecorations);
  let foundDecoration = false;

  decorations.between(line.from, line.to, () => {
    foundDecoration = true;
  });

  return foundDecoration;
}
