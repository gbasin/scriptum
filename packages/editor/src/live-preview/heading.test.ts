import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { livePreview } from "./extension.js";
import { headingPreviewDecorations } from "./heading.js";

describe("heading live preview", () => {
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

  it("keeps active heading line as raw markdown", () => {
    const state = EditorState.create({
      doc: "# Active\n## Inactive",
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    expect(hasDecorationOnLine(state, 1)).toBe(false);
    expect(hasDecorationOnLine(state, 2)).toBe(true);
  });

  it("updates heading decoration level after markdown level changes", () => {
    let state = EditorState.create({
      doc: "## Heading\nbody",
      selection: { anchor: "## Heading\n".length },
      extensions: [livePreview()],
    });

    expect(collectHeadingClasses(state).has("cm-livePreview-heading-h2")).toBe(
      true,
    );

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

  it("moves raw active line as cursor moves between heading lines", () => {
    let state = EditorState.create({
      doc: "# First\n# Second",
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    expect(hasDecorationOnLine(state, 1)).toBe(false);
    expect(hasDecorationOnLine(state, 2)).toBe(true);

    state = state.update({
      selection: { anchor: state.doc.line(2).from },
    }).state;

    expect(hasDecorationOnLine(state, 1)).toBe(true);
    expect(hasDecorationOnLine(state, 2)).toBe(false);
  });
});

function collectHeadingClasses(state: EditorState): Set<string> {
  const classes = new Set<string>();
  const decorations = state.field(headingPreviewDecorations);

  decorations.between(0, state.doc.length, (_from, _to, value) => {
    const spec = (
      value as {
        spec?: { class?: unknown; attributes?: { class?: unknown } };
      }
    ).spec;
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
