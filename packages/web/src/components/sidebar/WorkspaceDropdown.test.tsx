import type { Workspace } from "@scriptum/shared";
import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  formatLastAccessedLabel,
  WorkspaceDropdown,
} from "./WorkspaceDropdown";

const WORKSPACES: Workspace[] = [
  {
    id: "ws-alpha",
    slug: "alpha",
    name: "Alpha",
    role: "owner",
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-02T09:45:00.000Z",
    etag: "ws-alpha-v1",
  },
  {
    id: "ws-beta",
    slug: "beta",
    name: "Beta",
    role: "editor",
    createdAt: "2026-01-03T00:00:00.000Z",
    updatedAt: "2026-01-04T10:15:00.000Z",
    etag: "ws-beta-v1",
  },
];

describe("WorkspaceDropdown", () => {
  it("renders workspaces, last-accessed labels, and create option", () => {
    const html = renderToString(
      <WorkspaceDropdown
        activeWorkspaceId="ws-beta"
        lastAccessedByWorkspaceId={{ "ws-beta": "2026-01-05T11:00:00.000Z" }}
        onCreateWorkspace={() => undefined}
        onWorkspaceSelect={() => undefined}
        workspaces={WORKSPACES}
      />,
    );

    expect(html).toContain("Workspace switcher");
    expect(html).toContain("Workspace dropdown");
    expect(html).toContain("Alpha");
    expect(html).toContain("Beta");
    expect(html).toContain(formatLastAccessedLabel("2026-01-02T09:45:00.000Z"));
    expect(html).toContain(formatLastAccessedLabel("2026-01-05T11:00:00.000Z"));
    expect(html).toContain("Create new workspace");
  });

  it("returns fallback label when timestamp is missing or invalid", () => {
    expect(formatLastAccessedLabel(undefined)).toBe("Last accessed unknown");
    expect(formatLastAccessedLabel("invalid-timestamp")).toBe(
      "Last accessed unknown",
    );
  });
});
