import { CompletionContext } from "@codemirror/autocomplete";
import { EditorState } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { describe, expect, it, vi } from "vitest";
import { applySlashCommand, slashCommandCompletions } from "./extension.js";

describe("slash command autocomplete", () => {
  it("offers slash commands when typing slash", () => {
    const state = EditorState.create({ doc: "/" });
    const result = slashCommandCompletions(new CompletionContext(state, 1, false));

    expect(result?.from).toBe(0);
    expect(result?.to).toBe(1);
    expect(result?.options.map((option) => option.label)).toEqual([
      "/table",
      "/code",
      "/image",
      "/callout",
    ]);
  });

  it("filters slash command options by typed prefix", () => {
    const state = EditorState.create({ doc: "/c" });
    const result = slashCommandCompletions(new CompletionContext(state, 2, false));

    expect(result?.options.map((option) => option.label)).toEqual([
      "/code",
      "/callout",
    ]);
  });

  it("does not trigger for slash characters in url paths", () => {
    const doc = "https://example.com/path";
    const state = EditorState.create({ doc });
    const result = slashCommandCompletions(
      new CompletionContext(state, doc.length, false),
    );

    expect(result).toBeNull();
  });
});

describe("slash command insertion", () => {
  it("dispatches markdown template replacement for selected command", () => {
    const dispatch = vi.fn();
    applySlashCommand(
      { dispatch } as unknown as Pick<EditorView, "dispatch">,
      "image",
      8,
      14,
    );

    expect(dispatch).toHaveBeenCalledTimes(1);
    expect(dispatch).toHaveBeenCalledWith({
      changes: {
        from: 8,
        insert: "![alt text](https://example.com/image.png)",
        to: 14,
      },
      selection: {
        anchor: 10,
      },
    });
  });
});
