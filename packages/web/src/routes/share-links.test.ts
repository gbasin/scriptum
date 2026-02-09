import { describe, expect, it } from "vitest";
import {
  buildShareLinkUrl,
  shareUrlFromCreateShareLinkResponse,
} from "./share-links";

describe("shareUrlFromCreateShareLinkResponse", () => {
  it("uses snake_case url_once when provided", () => {
    expect(
      shareUrlFromCreateShareLinkResponse(
        {
          share_link: {
            url_once: "https://relay.scriptum.dev/share/abc123",
          },
        },
        "https://app.scriptum.dev",
      ),
    ).toBe("https://relay.scriptum.dev/share/abc123");
  });

  it("uses camelCase urlOnce when provided", () => {
    expect(
      shareUrlFromCreateShareLinkResponse(
        {
          shareLink: {
            urlOnce: "https://relay.scriptum.dev/share/def456",
          },
        },
        "https://app.scriptum.dev",
      ),
    ).toBe("https://relay.scriptum.dev/share/def456");
  });

  it("falls back to token when URL is absent", () => {
    expect(
      shareUrlFromCreateShareLinkResponse(
        {
          share_link: {
            token: "ghi789",
          },
        },
        "https://app.scriptum.dev",
      ),
    ).toBe(buildShareLinkUrl("ghi789", "https://app.scriptum.dev"));
  });

  it("returns null when payload does not contain a usable URL", () => {
    expect(shareUrlFromCreateShareLinkResponse({}, "https://app.scriptum.dev")).toBeNull();
  });
});
