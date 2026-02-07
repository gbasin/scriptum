import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { livePreview } from "./extension.js";
import { taskBlockquoteHrDecorations } from "./task-list.js";

describe("task, blockquote, and hr live preview", () => {
  it("applies blockquote styling on unfocused lines", () => {
    const source = ["active", "> quoted line"].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    const classes = collectTaskBlockquoteHrClasses(state);
    expect(classes.has("cm-livePreview-blockquote-line")).toBe(true);
    expect(hasDecorationOnLine(state, 1)).toBe(false);
    expect(hasDecorationOnLine(state, 2)).toBe(true);
  });

  it("keeps active blockquote lines raw markdown", () => {
    const source = ["> quoted line", "inactive"].join("\n");
    const state = EditorState.create({
      doc: source,
      selection: { anchor: 0 },
      extensions: [livePreview()],
    });

    expect(hasDecorationOnLine(state, 1)).toBe(false);
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
    expect(hasDecorationOnLine(inactiveHrState, 3)).toBe(true);

    const activeHrState = EditorState.create({
      doc: source,
      selection: { anchor: source.indexOf("---") },
      extensions: [livePreview()],
    });

    expect(hasDecorationOnLine(activeHrState, 3)).toBe(false);
  });
});

function collectTaskBlockquoteHrClasses(state: EditorState): Set<string> {
  const classes = new Set<string>();
  const decorations = state.field(taskBlockquoteHrDecorations);

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

function collectTaskCheckboxStates(state: EditorState): boolean[] {
  const states: boolean[] = [];
  const decorations = state.field(taskBlockquoteHrDecorations);

  decorations.between(0, state.doc.length, (_from, _to, value) => {
    const widget = (
      value.spec as { widget?: { kind?: unknown; checked?: unknown } }
    ).widget;
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

function hasDecorationOnLine(state: EditorState, lineNumber: number): boolean {
  const line = state.doc.line(lineNumber);
  const decorations = state.field(taskBlockquoteHrDecorations);
  let foundDecoration = false;

  decorations.between(line.from, line.to, () => {
    foundDecoration = true;
  });

  return foundDecoration;
}
