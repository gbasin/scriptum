import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { livePreview } from "./extension.js";
import { inlineLinkDecorations } from "./link.js";

interface InlineWidgetToken {
  kind: "image" | "link";
  href: string;
}

function collectInlineWidgets(state: EditorState): InlineWidgetToken[] {
  const widgets: InlineWidgetToken[] = [];
  const decorations = state.field(inlineLinkDecorations);

  decorations.between(0, state.doc.length, (_from, _to, value) => {
    const widget = (
      value.spec as {
        widget?: { kind?: unknown; href?: unknown; src?: unknown };
      }
    ).widget;
    if (!widget) {
      return;
    }
    if (widget.kind === "link" && typeof widget.href === "string") {
      widgets.push({ kind: "link", href: widget.href });
      return;
    }
    if (widget.kind === "image" && typeof widget.src === "string") {
      widgets.push({ kind: "image", href: widget.src });
    }
  });

  return widgets;
}

function stateWithSource(source: string): EditorState {
  return EditorState.create({
    doc: source,
    selection: { anchor: 0 },
    extensions: [livePreview()],
  });
}

describe("live preview security", () => {
  it("rejects javascript and data urls for markdown links", () => {
    const state = stateWithSource(
      [
        "active",
        "[unsafe-js](javascript:alert(1))",
        "[unsafe-data](data:text/html,boom)",
      ].join("\n"),
    );

    const widgets = collectInlineWidgets(state);
    expect(widgets).toEqual([]);
  });

  it("rejects javascript and data urls for images including svg payloads", () => {
    const state = stateWithSource(
      [
        "active",
        "![x](javascript:alert(1))",
        "![x](data:image/svg+xml,<svg/onload=alert(1)>)",
      ].join("\n"),
    );

    const widgets = collectInlineWidgets(state);
    expect(widgets).toEqual([]);
  });

  it("rejects javascript autolinks and keeps safe https links", () => {
    const state = stateWithSource(
      ["active", "<javascript:alert(1)>", "<https://scriptum.dev/docs>"].join(
        "\n",
      ),
    );

    const widgets = collectInlineWidgets(state);
    expect(widgets).toEqual([
      { kind: "link", href: "https://scriptum.dev/docs" },
    ]);
  });

  it("does not render raw html script tags or event handler payloads as preview widgets", () => {
    const state = stateWithSource(
      [
        "active",
        "<script>alert(1)</script>",
        '<img src="x" onerror="alert(1)">',
      ].join("\n"),
    );

    const widgets = collectInlineWidgets(state);
    expect(widgets).toEqual([]);
  });
});
