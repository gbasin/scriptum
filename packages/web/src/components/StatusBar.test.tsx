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
        pendingUpdates={0}
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
        pendingUpdates={19}
      />,
    );
    const normalized = html.replaceAll("<!-- -->", "");

    expect(normalized).toContain("Sync: Offline");
    expect(normalized).toContain('data-sync-color="red"');
    expect(normalized).toContain("Pending: 19");
  });

  it("shows reconnect progress details while reconnecting", () => {
    const html = renderToString(
      <StatusBar
        syncState="reconnecting"
        cursor={{ line: 1, ch: 4 }}
        activeEditors={2}
        pendingUpdates={356}
        reconnectProgress={{ syncedUpdates: 847, totalUpdates: 1203 }}
      />,
    );
    const normalized = html.replaceAll("<!-- -->", "");

    expect(normalized).toContain("Sync: Reconnecting");
    expect(normalized).toContain('data-sync-color="yellow"');
    expect(normalized).toContain("Pending: 356");
    expect(normalized).toContain("Syncing... 847/1203 updates");
  });
});
