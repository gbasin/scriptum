// @vitest-environment jsdom

import type { Document, Workspace } from "@scriptum/shared";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useDocumentsStore } from "../store/documents";
import { useWorkspaceStore } from "../store/workspace";
import { buildIncomingBacklinks, Layout } from "./Layout";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

function makeWorkspace(): Workspace {
  return {
    createdAt: "2026-01-01T00:00:00.000Z",
    etag: "ws-alpha-v1",
    id: "ws-alpha",
    name: "Alpha Workspace",
    role: "owner",
    slug: "alpha",
    updatedAt: "2026-01-02T00:00:00.000Z",
  };
}

function makeDocument(
  overrides: Partial<Document> & { id: string; path: string },
): Document {
  return {
    archivedAt: null,
    createdAt: "2026-01-01T00:00:00.000Z",
    deletedAt: null,
    etag: `etag-${overrides.id}`,
    headSeq: 0,
    tags: [],
    title: overrides.path.split("/").pop() ?? "",
    updatedAt: "2026-01-03T00:00:00.000Z",
    workspaceId: "ws-alpha",
    ...overrides,
  };
}

beforeEach(() => {
  useWorkspaceStore.getState().reset();
  useDocumentsStore.getState().reset();

  const workspace = makeWorkspace();
  useWorkspaceStore.getState().upsertWorkspace(workspace);
  useWorkspaceStore.getState().setActiveWorkspaceId(workspace.id);

  const documents = [
    makeDocument({
      id: "doc-a",
      path: "docs/auth.md",
      tags: ["auth"],
      title: "Auth",
    }),
    makeDocument({
      id: "doc-b",
      path: "docs/search.md",
      tags: ["search"],
      title: "Search",
    }),
  ];

  useDocumentsStore.getState().setDocuments(documents);
  useDocumentsStore
    .getState()
    .setActiveDocumentForWorkspace(workspace.id, documents[0].id);
});

afterEach(() => {
  document.body.innerHTML = "";
  globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
});

describe("Layout search panel integration", () => {
  it("opens search panel with Cmd+Shift+F and replaces document tree", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <MemoryRouter initialEntries={["/workspace/ws-alpha"]}>
          <Routes>
            <Route element={<Layout />}>
              <Route path="/workspace/:workspaceId" element={<div />} />
            </Route>
          </Routes>
        </MemoryRouter>,
      );
    });

    expect(container.querySelector("[data-testid=\"document-tree\"]")).not.toBeNull();
    expect(container.querySelector("[data-testid=\"search-panel\"]")).toBeNull();

    act(() => {
      window.dispatchEvent(
        new KeyboardEvent("keydown", {
          bubbles: true,
          key: "f",
          metaKey: true,
          shiftKey: true,
        }),
      );
    });

    expect(container.querySelector("[data-testid=\"search-panel\"]")).not.toBeNull();
    expect(container.querySelector("[data-testid=\"document-tree\"]")).toBeNull();

    const closeButton = container.querySelector(
      "[data-testid=\"search-panel-close\"]",
    ) as HTMLButtonElement | null;
    act(() => {
      closeButton?.click();
    });

    expect(container.querySelector("[data-testid=\"search-panel\"]")).toBeNull();
    expect(container.querySelector("[data-testid=\"document-tree\"]")).not.toBeNull();

    act(() => {
      root.unmount();
    });
  });
});

function OutlineFixture() {
  return (
    <article data-testid="outline-fixture">
      <h1>Document Summary</h1>
      <h2>Overview</h2>
      <h2>Implementation</h2>
    </article>
  );
}

describe("Layout outline panel", () => {
  it("renders heading outline, toggles panel visibility, and scrolls on click", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <MemoryRouter initialEntries={["/workspace/ws-alpha"]}>
          <Routes>
            <Route element={<Layout />}>
              <Route path="/workspace/:workspaceId" element={<OutlineFixture />} />
            </Route>
          </Routes>
        </MemoryRouter>,
      );
    });

    const outlinePanel = container.querySelector("[data-testid=\"outline-panel\"]");
    expect(outlinePanel).not.toBeNull();
    expect(outlinePanel?.textContent).toContain("Document Summary");
    expect(outlinePanel?.textContent).toContain("Overview");
    expect(outlinePanel?.textContent).toContain("Implementation");

    const targetHeading = Array.from(container.querySelectorAll("h2")).find(
      (heading) => heading.textContent === "Implementation",
    ) as HTMLHeadingElement | undefined;
    expect(targetHeading).toBeDefined();
    const scrollIntoViewSpy = vi.fn();
    if (targetHeading) {
      targetHeading.scrollIntoView = scrollIntoViewSpy;
    }

    const implementationButton = Array.from(
      container.querySelectorAll<HTMLButtonElement>("[data-testid^=\"outline-heading-\"]"),
    ).find((button) => button.textContent?.includes("Implementation"));
    expect(implementationButton).toBeDefined();
    act(() => {
      implementationButton?.click();
    });
    expect(scrollIntoViewSpy).toHaveBeenCalledTimes(1);

    const toggleButton = container.querySelector(
      "[data-testid=\"outline-panel-toggle\"]",
    ) as HTMLButtonElement | null;
    act(() => {
      toggleButton?.click();
    });

    expect(container.querySelector("[data-testid=\"outline-panel\"]")).toBeNull();
    const reopenButton = container.querySelector(
      "[data-testid=\"outline-panel-toggle\"]",
    ) as HTMLButtonElement | null;
    expect(reopenButton?.textContent).toContain("Show Outline");

    act(() => {
      reopenButton?.click();
    });
    expect(container.querySelector("[data-testid=\"outline-panel\"]")).not.toBeNull();

    act(() => {
      root.unmount();
    });
  });

  it("highlights the current section based on scroll position", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <MemoryRouter initialEntries={["/workspace/ws-alpha"]}>
          <Routes>
            <Route element={<Layout />}>
              <Route path="/workspace/:workspaceId" element={<OutlineFixture />} />
            </Route>
          </Routes>
        </MemoryRouter>,
      );
    });

    const fixture = container.querySelector(
      "[data-testid=\"outline-fixture\"]",
    ) as HTMLElement | null;
    const headings = Array.from(
      fixture?.querySelectorAll<HTMLHeadingElement>("h1, h2") ?? [],
    );
    expect(headings.length).toBeGreaterThanOrEqual(3);
    const [summaryHeading, overviewHeading, implementationHeading] = headings;

    summaryHeading.getBoundingClientRect = () =>
      ({
        bottom: -120,
        height: 20,
        left: 0,
        right: 200,
        top: -140,
        width: 200,
      }) as DOMRect;
    overviewHeading.getBoundingClientRect = () =>
      ({
        bottom: 70,
        height: 20,
        left: 0,
        right: 200,
        top: 50,
        width: 200,
      }) as DOMRect;
    implementationHeading.getBoundingClientRect = () =>
      ({
        bottom: 290,
        height: 20,
        left: 0,
        right: 200,
        top: 270,
        width: 200,
      }) as DOMRect;

    act(() => {
      window.dispatchEvent(new Event("scroll"));
    });

    const overviewButton = Array.from(
      container.querySelectorAll<HTMLButtonElement>("[data-testid^=\"outline-heading-\"]"),
    ).find((button) => button.textContent?.includes("Overview"));
    expect(overviewButton?.getAttribute("data-active")).toBe("true");

    act(() => {
      root.unmount();
    });
  });
});

describe("Layout backlinks panel", () => {
  it("resolves incoming wiki links by path, filename, and title", () => {
    const documents = [
      makeDocument({
        id: "doc-auth",
        path: "docs/auth.md",
        title: "Auth",
      }),
      makeDocument({
        id: "doc-path",
        path: "notes/by-path.md",
        title: "By Path",
        bodyMd: "See [[docs/auth.md]] for details.",
      }),
      makeDocument({
        id: "doc-file",
        path: "notes/by-file.md",
        title: "By File",
        bodyMd: "Related setup lives in [[auth]].",
      }),
      makeDocument({
        id: "doc-title",
        path: "notes/by-title.md",
        title: "By Title",
        bodyMd: "Context in [[Auth|Authentication design]].",
      }),
      makeDocument({
        id: "doc-other",
        path: "notes/other.md",
        title: "Other",
        bodyMd: "No backlink match here.",
      }),
    ];

    const backlinks = buildIncomingBacklinks(documents, "doc-auth");
    expect(backlinks).toEqual([
      {
        snippet: "[[auth]]",
        sourceDocumentId: "doc-file",
        sourcePath: "notes/by-file.md",
        sourceTitle: "By File",
      },
      {
        snippet: "[[docs/auth.md]]",
        sourceDocumentId: "doc-path",
        sourcePath: "notes/by-path.md",
        sourceTitle: "By Path",
      },
      {
        snippet: "[[Auth|Authentication design]]",
        sourceDocumentId: "doc-title",
        sourcePath: "notes/by-title.md",
        sourceTitle: "By Title",
      },
    ]);
  });

  it("navigates to the source document when a backlink is clicked", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const workspace = makeWorkspace();
    useDocumentsStore.getState().setDocuments([
      makeDocument({
        id: "doc-auth",
        path: "docs/auth.md",
        title: "Auth",
      }),
      makeDocument({
        id: "doc-notes",
        path: "notes/overview.md",
        title: "Overview",
        bodyMd: "Read [[Auth]] first.",
      }),
    ]);
    useDocumentsStore
      .getState()
      .setActiveDocumentForWorkspace(workspace.id, "doc-auth");

    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(
        <MemoryRouter initialEntries={["/workspace/ws-alpha/document/doc-auth"]}>
          <Routes>
            <Route element={<Layout />}>
              <Route path="/workspace/:workspaceId/document/:docId" element={<div />} />
            </Route>
          </Routes>
        </MemoryRouter>,
      );
    });

    const backlinkButton = container.querySelector(
      "[data-testid=\"backlink-item-doc-notes\"]",
    ) as HTMLButtonElement | null;
    expect(backlinkButton?.textContent).toContain("Overview");

    act(() => {
      backlinkButton?.click();
    });

    expect(
      useDocumentsStore.getState().activeDocumentIdByWorkspace[workspace.id],
    ).toBe("doc-notes");

    act(() => {
      root.unmount();
    });
  });
});
