import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import {
  activeLines,
  getMarkdownNodes,
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
});
