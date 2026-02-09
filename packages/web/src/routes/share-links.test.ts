import { describe, expect, it } from "vitest";
import {
  buildShareLinkUrl,
  shareLinkFromCreateShareLinkResponse,
  shareLinksFromListResponse,
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
    expect(
      shareUrlFromCreateShareLinkResponse({}, "https://app.scriptum.dev"),
    ).toBeNull();
  });
});

describe("shareLinkFromCreateShareLinkResponse", () => {
  it("parses a share link envelope with snake_case fields", () => {
    expect(
      shareLinkFromCreateShareLinkResponse({
        share_link: {
          id: "share-1",
          target_type: "document",
          target_id: "doc-1",
          permission: "edit",
          expires_at: null,
          max_uses: 5,
          use_count: 1,
          disabled: false,
          created_at: "2026-02-09T00:00:00.000Z",
          revoked_at: null,
          etag: "etag-1",
          url_once: "https://relay.scriptum.dev/share/token-1",
        },
      }),
    ).toEqual({
      id: "share-1",
      targetType: "document",
      targetId: "doc-1",
      permission: "edit",
      expiresAt: null,
      maxUses: 5,
      useCount: 1,
      disabled: false,
      createdAt: "2026-02-09T00:00:00.000Z",
      revokedAt: null,
      etag: "etag-1",
      urlOnce: "https://relay.scriptum.dev/share/token-1",
    });
  });
});

describe("shareLinksFromListResponse", () => {
  it("parses and sorts list payload items by createdAt descending", () => {
    expect(
      shareLinksFromListResponse({
        items: [
          {
            id: "share-old",
            target_type: "workspace",
            target_id: "ws-1",
            permission: "view",
            expires_at: null,
            max_uses: null,
            use_count: 0,
            disabled: false,
            created_at: "2026-02-08T00:00:00.000Z",
            revoked_at: null,
            etag: "etag-old",
            url_once: "",
          },
          {
            id: "share-new",
            target_type: "workspace",
            target_id: "ws-1",
            permission: "view",
            expires_at: null,
            max_uses: null,
            use_count: 0,
            disabled: false,
            created_at: "2026-02-09T00:00:00.000Z",
            revoked_at: null,
            etag: "etag-new",
            url_once: "",
          },
        ],
      }).map((item) => item.id),
    ).toEqual(["share-new", "share-old"]);
  });
});
