import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { StatusBar } from "./StatusBar";

describe("StatusBar", () => {
  it("renders sync state, cursor position, and active editor count", () => {
    const html = renderToString(
      <StatusBar
        syncState="synced"
        cursor={{ line: 4, ch: 9 }}
        activeEditors={3}
      />,
    );
    const normalized = html.replaceAll("<!-- -->", "");

    expect(normalized).toContain("Sync: Synced");
    expect(normalized).toContain("Ln 5, Col 10");
    expect(normalized).toContain("Editors: 3");
    expect(normalized).toContain('data-sync-color="green"');
  });

  it("shows Offline with red indicator", () => {
    const html = renderToString(
      <StatusBar
        syncState="offline"
        cursor={{ line: 0, ch: 0 }}
        activeEditors={1}
      />,
    );
    const normalized = html.replaceAll("<!-- -->", "");

    expect(normalized).toContain("Sync: Offline");
    expect(normalized).toContain('data-sync-color="red"');
  });
});
