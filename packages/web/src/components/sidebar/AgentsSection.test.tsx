import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { PeerPresence } from "../../store/presence";
import { AgentsSection, activityStatusFromLastSeen } from "./AgentsSection";

const AGENT: PeerPresence = {
  name: "Claude Agent",
  type: "agent",
  activeDocumentPath: "docs/spec.md",
  cursor: null,
  lastSeenAt: "2026-02-07T00:00:00.000Z",
  color: "#60a5fa",
};

const HUMAN: PeerPresence = {
  name: "Alice",
  type: "human",
  activeDocumentPath: "docs/readme.md",
  cursor: null,
  lastSeenAt: "2026-02-07T00:00:00.000Z",
  color: "#34d399",
};

describe("activityStatusFromLastSeen", () => {
  it("returns active when heartbeat is recent", () => {
    expect(
      activityStatusFromLastSeen(
        "2026-02-07T00:00:30.000Z",
        Date.parse("2026-02-07T00:01:00.000Z"),
      ),
    ).toBe("active");
  });

  it("returns idle when heartbeat is stale or invalid", () => {
    expect(
      activityStatusFromLastSeen(
        "2026-02-07T00:00:00.000Z",
        Date.parse("2026-02-07T00:01:30.000Z"),
      ),
    ).toBe("idle");
    expect(activityStatusFromLastSeen("not-a-timestamp", Date.now())).toBe(
      "idle",
    );
  });
});

describe("AgentsSection", () => {
  it("renders agent name, status, and active document path", () => {
    const html = renderToString(
      <AgentsSection
        nowMs={Date.parse("2026-02-07T00:00:20.000Z")}
        peers={[HUMAN, AGENT]}
      />,
    );

    expect(html).toContain("Agents");
    expect(html).toContain("Claude Agent");
    expect(html).toContain("Status:");
    expect(html).toContain("active");
    expect(html).toContain("Editing:");
    expect(html).toContain("docs/spec.md");
    expect(html).not.toContain("Alice");
  });

  it("renders empty state when no agents are present", () => {
    const html = renderToString(<AgentsSection peers={[HUMAN]} />);
    expect(html).toContain("No active agents.");
  });
});
