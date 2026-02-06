import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import {
  activeLines,
  getMarkdownNodes,
  headingPreviewDecorations,
  inlineEmphasisDecorations,
  inlineLinkDecorations,
  isLineActive,
  livePreview,
  tablePreviewDecorations,
  taskBlockquoteHrDecorations,
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

  it("renders bold/italic/strikethrough on unfocused lines", () => {
    const source = ["*active line*", "**bold** and _italic_ and ~~strike~~"].join(
      "\n",
    );

    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    const classes = collectInlineClasses(state);
    expect(classes.has("cm-livePreview-strong")).toBe(true);
    expect(classes.has("cm-livePreview-emphasis")).toBe(true);
    expect(classes.has("cm-livePreview-strikethrough")).toBe(true);

    expect(hasInlineDecorationOnLine(state, 1)).toBe(false);
    expect(hasInlineDecorationOnLine(state, 2)).toBe(true);
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

  it("keeps active line raw for inline emphasis markers", () => {
    const source = ["plain", "**bold** and _italic_ and ~~strike~~"].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: source.indexOf("**bold**") },
      extensions: [livePreview()],
    });

    expect(hasInlineDecorationOnLine(state, 2)).toBe(false);
  });

  it("renders markdown links and autolinks on unfocused lines", () => {
    const source = [
      "active line",
      "[Scriptum](https://scriptum.dev) and <https://example.com>",
    ].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    const widgets = collectInlineWidgetKinds(state);
    expect(widgets).toEqual(["link", "link"]);
    expect(hasInlineLinkDecorationOnLine(state, 1)).toBe(false);
    expect(hasInlineLinkDecorationOnLine(state, 2)).toBe(true);
  });

  it("renders image previews on unfocused lines", () => {
    const source = ["active", "![Alt text](https://example.com/image.png)"].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    const widgets = collectInlineWidgetKinds(state);
    expect(widgets).toEqual(["image"]);
    expect(hasInlineLinkDecorationOnLine(state, 2)).toBe(true);
  });

  it("keeps active line raw for link and image markdown", () => {
    const source = "[raw](https://example.com) ![img](https://example.com/a.png)\nline";
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    expect(hasInlineLinkDecorationOnLine(state, 1)).toBe(false);
  });

  it("renders GFM tables with alignment metadata on unfocused lines", () => {
    const source = [
      "active",
      "| Name | Role | Score |",
      "| :--- | :---: | ---: |",
      "| Ada | Agent | 42 |",
    ].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    const tables = collectTableWidgets(state);
    expect(tables).toEqual([
      {
        alignments: ["left", "center", "right"],
        headers: ["Name", "Role", "Score"],
      },
    ]);
    expect(hasTableDecorationOnLine(state, 2)).toBe(true);
  });

  it("keeps active line raw for markdown table syntax", () => {
    const source = [
      "| Name | Role |",
      "| :--- | :--: |",
      "| Ada | Agent |",
    ].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: source.indexOf("| Name |") },
      extensions: [livePreview()],
    });

    expect(hasTableDecorationOnLine(state, 1)).toBe(false);
    expect(hasTableDecorationOnLine(state, 2)).toBe(false);
  });

  it("renders blockquote styling on unfocused lines", () => {
    const source = ["active", "> quoted line"].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    const classes = collectTaskBlockquoteHrClasses(state);
    expect(classes.has("cm-livePreview-blockquote-line")).toBe(true);
    expect(hasTaskBlockquoteHrDecorationOnLine(state, 1)).toBe(false);
    expect(hasTaskBlockquoteHrDecorationOnLine(state, 2)).toBe(true);
  });

  it("renders task list checkbox widgets on unfocused lines", () => {
    const source = ["active", "- [ ] todo", "- [x] done"].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    const widgets = collectTaskCheckboxStates(state);
    expect(widgets).toEqual([false, true]);
  });

  it("renders horizontal rules on unfocused lines and keeps active line raw", () => {
    const source = ["active", "", "---"].join("\n");
    const inactiveHrState = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });
    expect(collectTaskBlockquoteHrWidgetKinds(inactiveHrState)).toContain(
      "horizontal-rule",
    );
    expect(hasTaskBlockquoteHrDecorationOnLine(inactiveHrState, 3)).toBe(true);

    const activeHrState = EditorState.create({
      doc: source,
      selection: { anchor: source.indexOf("---") },
      extensions: [livePreview()],
    });
    expect(hasTaskBlockquoteHrDecorationOnLine(activeHrState, 3)).toBe(false);
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

function collectInlineClasses(state: EditorState): Set<string> {
  const classes = new Set<string>();
  const decorations = state.field(inlineEmphasisDecorations);

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
      if (token.startsWith("cm-livePreview-")) {
        classes.add(token);
      }
    }
  });

  return classes;
}

function hasInlineDecorationOnLine(state: EditorState, lineNumber: number): boolean {
  const line = state.doc.line(lineNumber);
  const decorations = state.field(inlineEmphasisDecorations);
  let foundDecoration = false;

  decorations.between(line.from, line.to, () => {
    foundDecoration = true;
  });

  return foundDecoration;
}

function collectInlineWidgetKinds(state: EditorState): string[] {
  const kinds: string[] = [];
  const decorations = state.field(inlineLinkDecorations);

  decorations.between(0, state.doc.length, (_from, _to, value) => {
    const widget = (value.spec as { widget?: { kind?: unknown } }).widget;
    if (!widget || (widget.kind !== "link" && widget.kind !== "image")) {
      return;
    }
    kinds.push(widget.kind);
  });

  return kinds;
}

function hasInlineLinkDecorationOnLine(state: EditorState, lineNumber: number): boolean {
  const line = state.doc.line(lineNumber);
  const decorations = state.field(inlineLinkDecorations);
  let foundDecoration = false;

  decorations.between(line.from, line.to, () => {
    foundDecoration = true;
  });

  return foundDecoration;
}

function collectTableWidgets(state: EditorState): Array<{
  headers: string[];
  alignments: string[];
}> {
  const tables: Array<{ headers: string[]; alignments: string[] }> = [];
  const decorations = state.field(tablePreviewDecorations);

  decorations.between(0, state.doc.length, (_from, _to, value) => {
    const widget = (value.spec as {
      widget?: {
        kind?: unknown;
        headers?: unknown;
        alignments?: unknown;
      };
    }).widget;
    if (!widget || widget.kind !== "table") {
      return;
    }
    if (!Array.isArray(widget.headers) || !Array.isArray(widget.alignments)) {
      return;
    }

    tables.push({
      alignments: widget.alignments.filter(
        (alignment): alignment is string => typeof alignment === "string",
      ),
      headers: widget.headers.filter(
        (header): header is string => typeof header === "string",
      ),
    });
  });

  return tables;
}

function hasTableDecorationOnLine(state: EditorState, lineNumber: number): boolean {
  const line = state.doc.line(lineNumber);
  const decorations = state.field(tablePreviewDecorations);
  let foundDecoration = false;

  decorations.between(line.from, line.to, () => {
    foundDecoration = true;
  });

  return foundDecoration;
}

function collectTaskBlockquoteHrClasses(state: EditorState): Set<string> {
  const classes = new Set<string>();
  const decorations = state.field(taskBlockquoteHrDecorations);

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
      if (token.startsWith("cm-livePreview-")) {
        classes.add(token);
      }
    }
  });

  return classes;
}

function collectTaskCheckboxStates(state: EditorState): boolean[] {
  const states: boolean[] = [];
  const decorations = state.field(taskBlockquoteHrDecorations);

  decorations.between(0, state.doc.length, (_from, _to, value) => {
    const widget = (value.spec as { widget?: { kind?: unknown; checked?: unknown } }).widget;
    if (!widget || widget.kind !== "task-checkbox") {
      return;
    }
    states.push(Boolean(widget.checked));
  });

  return states;
}

function collectTaskBlockquoteHrWidgetKinds(state: EditorState): string[] {
  const kinds: string[] = [];
  const decorations = state.field(taskBlockquoteHrDecorations);

  decorations.between(0, state.doc.length, (_from, _to, value) => {
    const widget = (value.spec as { widget?: { kind?: unknown } }).widget;
    if (!widget || typeof widget.kind !== "string") {
      return;
    }
    kinds.push(widget.kind);
  });

  return kinds;
}

function hasTaskBlockquoteHrDecorationOnLine(
  state: EditorState,
  lineNumber: number,
): boolean {
  const line = state.doc.line(lineNumber);
  const decorations = state.field(taskBlockquoteHrDecorations);
  let foundDecoration = false;

  decorations.between(line.from, line.to, () => {
    foundDecoration = true;
  });

  return foundDecoration;
}
