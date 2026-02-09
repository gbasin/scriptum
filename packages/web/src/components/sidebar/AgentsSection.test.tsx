// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToString } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PeerPresence } from "../../store/presence";
import { AgentsSection, activityStatusFromLastSeen } from "./AgentsSection";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

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

afterEach(() => {
  document.body.innerHTML = "";
  globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
  vi.useRealTimers();
});

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

  it("refreshes activity status over time without external rerenders", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    vi.useFakeTimers();
    vi.setSystemTime(Date.parse("2026-02-07T00:01:00.000Z"));
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    const tickingAgent: PeerPresence = {
      ...AGENT,
      lastSeenAt: "2026-02-07T00:00:10.000Z",
    };

    act(() => {
      root.render(<AgentsSection peers={[tickingAgent]} />);
    });

    const statusNode = container.querySelector(
      '[data-testid="sidebar-agent-status-Claude Agent"]',
    );
    expect(statusNode?.textContent).toContain("active");

    act(() => {
      vi.advanceTimersByTime(15_000);
    });

    expect(statusNode?.textContent).toContain("idle");

    act(() => {
      root.unmount();
    });
  });
});
