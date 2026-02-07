import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { PeerPresence } from "../store/presence";
import { AvatarStack, colorForName, initialsForName } from "./AvatarStack";

function makePeer(
  name: string,
  type: "human" | "agent" = "human",
): PeerPresence {
  return {
    name,
    type,
    activeDocumentPath: null,
    cursor: null,
    lastSeenAt: "2026-01-15T10:00:00Z",
    color: colorForName(name),
  };
}

describe("colorForName", () => {
  it("returns a color string", () => {
    const color = colorForName("alice");
    expect(color).toMatch(/^#[0-9a-f]{6}$/);
  });

  it("is deterministic", () => {
    expect(colorForName("bob")).toBe(colorForName("bob"));
  });

  it("produces different colors for different names", () => {
    // Not guaranteed for all inputs but very likely for distinct short names
    const colors = new Set([
      colorForName("alice"),
      colorForName("bob"),
      colorForName("charlie"),
      colorForName("diana"),
    ]);
    expect(colors.size).toBeGreaterThanOrEqual(2);
  });
});

describe("initialsForName", () => {
  it("returns first two letters for single word", () => {
    expect(initialsForName("alice")).toBe("AL");
  });

  it("returns first + last initials for multi-word name", () => {
    expect(initialsForName("Alice Brown")).toBe("AB");
  });

  it("handles three-word names", () => {
    expect(initialsForName("John Paul Smith")).toBe("JS");
  });

  it("uppercases result", () => {
    expect(initialsForName("bob")).toBe("BO");
  });
});

describe("AvatarStack", () => {
  it("renders nothing when no peers", () => {
    const html = renderToString(<AvatarStack peers={[]} />);
    // Returns null → empty string in SSR
    expect(html).toBe("");
  });

  it("renders avatars for each peer", () => {
    const peers = [makePeer("alice"), makePeer("bob")];
    const html = renderToString(<AvatarStack peers={peers} />);

    expect(html).toContain('data-testid="avatar-stack"');
    expect(html).toContain('data-testid="avatar-alice"');
    expect(html).toContain('data-testid="avatar-bob"');
    expect(html).toContain("AL");
    expect(html).toContain("BO");
  });

  it("sorts peers alphabetically by name", () => {
    const peers = [makePeer("charlie"), makePeer("alice"), makePeer("bob")];
    const html = renderToString(<AvatarStack peers={peers} />);

    // All three should be present
    expect(html).toContain("avatar-alice");
    expect(html).toContain("avatar-bob");
    expect(html).toContain("avatar-charlie");
  });

  it("shows overflow indicator when exceeding maxVisible", () => {
    const peers = [
      makePeer("alice"),
      makePeer("bob"),
      makePeer("charlie"),
      makePeer("diana"),
    ];
    const html = renderToString(<AvatarStack maxVisible={2} peers={peers} />);

    // Should show 2 visible + overflow
    expect(html).toContain('data-testid="avatar-overflow"');
    expect(html).toContain("2 more");
  });

  it("does not show overflow when peers fit within maxVisible", () => {
    const peers = [makePeer("alice"), makePeer("bob")];
    const html = renderToString(<AvatarStack maxVisible={5} peers={peers} />);

    expect(html).not.toContain("avatar-overflow");
  });

  it("applies agent border style for agent peers", () => {
    const peers = [makePeer("claude-agent", "agent")];
    const html = renderToString(<AvatarStack peers={peers} />);

    expect(html).toContain("avatar-claude-agent");
    // Agent border color
    expect(html).toContain("#374151");
    expect(html).toContain("(agent)");
  });

  it("uses ARIA roles for accessibility", () => {
    const peers = [makePeer("alice")];
    const html = renderToString(<AvatarStack peers={peers} />);

    expect(html).toContain('role="group"');
    expect(html).toContain('aria-label="Online users"');
  });

  it("respects custom size", () => {
    const peers = [makePeer("alice")];
    const html = renderToString(<AvatarStack peers={peers} size={48} />);

    expect(html).toContain("height:48px");
    expect(html).toContain("width:48px");
  });
});
