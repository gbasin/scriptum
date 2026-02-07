import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { livePreview } from "./extension.js";
import { inlineEmphasisDecorations } from "./emphasis.js";

describe("emphasis live preview", () => {
  it("renders italic emphasis on unfocused lines", () => {
    const source = ["active", "*italic* and _also italic_"].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    const classes = collectInlineClasses(state);
    expect(classes.has("cm-livePreview-emphasis")).toBe(true);
    expect(hasInlineDecorationOnLine(state, 2)).toBe(true);
  });

  it("renders bold emphasis on unfocused lines", () => {
    const source = ["active", "**bold**"].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    const classes = collectInlineClasses(state);
    expect(classes.has("cm-livePreview-strong")).toBe(true);
    expect(classes.has("cm-livePreview-emphasis")).toBe(false);
    expect(classes.has("cm-livePreview-strikethrough")).toBe(false);
  });

  it("renders strikethrough emphasis on unfocused lines", () => {
    const source = ["active", "~~done~~"].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    const classes = collectInlineClasses(state);
    expect(classes.has("cm-livePreview-strikethrough")).toBe(true);
    expect(classes.has("cm-livePreview-emphasis")).toBe(false);
    expect(classes.has("cm-livePreview-strong")).toBe(false);
  });

  it("supports nested emphasis on unfocused lines", () => {
    const source = ["active", "**bold _nested_**"].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    const classes = collectInlineClasses(state);
    expect(classes.has("cm-livePreview-strong")).toBe(true);
    expect(classes.has("cm-livePreview-emphasis")).toBe(true);
  });

  it("keeps active line raw for emphasis markdown markers", () => {
    const source = ["plain", "**bold** and _italic_ and ~~strike~~"].join(
      "\n",
    );
    const state = EditorState.create({
      doc: source,
      selection: { anchor: source.indexOf("**bold**") },
      extensions: [livePreview()],
    });

    expect(hasInlineDecorationOnLine(state, 2)).toBe(false);
  });
});

function collectInlineClasses(state: EditorState): Set<string> {
  const classes = new Set<string>();
  const decorations = state.field(inlineEmphasisDecorations);

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
      if (token.startsWith("cm-livePreview-")) {
        classes.add(token);
      }
    }
  });

  return classes;
}

function hasInlineDecorationOnLine(
  state: EditorState,
  lineNumber: number,
): boolean {
  const line = state.doc.line(lineNumber);
  const decorations = state.field(inlineEmphasisDecorations);
  let foundDecoration = false;

  decorations.between(line.from, line.to, () => {
    foundDecoration = true;
  });

  return foundDecoration;
}
