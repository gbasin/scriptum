// @vitest-environment jsdom

import { describe, expect, it } from "vitest";
import { isNewDocumentShortcut } from "./Layout";

describe("isNewDocumentShortcut", () => {
  it("matches Cmd/Ctrl+N without modifiers", () => {
    expect(
      isNewDocumentShortcut(
        new KeyboardEvent("keydown", { key: "n", metaKey: true }),
      ),
    ).toBe(true);
    expect(
      isNewDocumentShortcut(
        new KeyboardEvent("keydown", { key: "N", ctrlKey: true }),
      ),
    ).toBe(true);
  });

  it("ignores unrelated keys and modifier combinations", () => {
    expect(
      isNewDocumentShortcut(
        new KeyboardEvent("keydown", {
          altKey: true,
          key: "n",
          metaKey: true,
        }),
      ),
    ).toBe(false);
    expect(
      isNewDocumentShortcut(
        new KeyboardEvent("keydown", {
          key: "n",
          metaKey: true,
          shiftKey: true,
        }),
      ),
    ).toBe(false);
    expect(
      isNewDocumentShortcut(
        new KeyboardEvent("keydown", { key: "k", metaKey: true }),
      ),
    ).toBe(false);
  });
});
