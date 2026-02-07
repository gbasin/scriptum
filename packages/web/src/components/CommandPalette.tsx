import type { Document, Workspace } from "@scriptum/shared";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";

const MAX_RECENT_DOCUMENTS = 5;

export type CommandPaletteItemKind = "command" | "file" | "recent";
export type CommandPaletteAction = "create-workspace" | "navigate";

export interface CommandPaletteItem {
  action: CommandPaletteAction;
  id: string;
  kind: CommandPaletteItemKind;
  route: string | null;
  subtitle: string;
  title: string;
}

export interface BuildCommandPaletteItemsArgs {
  activeWorkspaceId: string | null;
  documents: Document[];
  openDocumentIds: string[];
  workspaces: Workspace[];
}

export interface CommandPaletteProps {
  activeWorkspaceId: string | null;
  documents: Document[];
  onCreateWorkspace: () => void;
  openDocumentIds: string[];
  workspaces: Workspace[];
}

function documentRoute(document: Document): string {
  return `/workspace/${encodeURIComponent(document.workspaceId)}/document/${encodeURIComponent(document.id)}`;
}

function workspaceRoute(workspaceId: string): string {
  return `/workspace/${encodeURIComponent(workspaceId)}`;
}

function normalizeQuery(value: string): string[] {
  return value
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter((token) => token.length > 0);
}

function itemSearchText(item: CommandPaletteItem): string {
  return `${item.kind} ${item.title} ${item.subtitle}`.toLowerCase();
}

function recentDocuments(
  documents: Document[],
  openDocumentIds: string[],
  activeWorkspaceId: string | null,
): Document[] {
  const documentById = new Map(
    documents.map((document) => [document.id, document]),
  );
  const seen = new Set<string>();
  const recent: Document[] = [];

  for (let index = openDocumentIds.length - 1; index >= 0; index -= 1) {
    const documentId = openDocumentIds[index];
    if (seen.has(documentId)) {
      continue;
    }

    const document = documentById.get(documentId);
    if (!document) {
      continue;
    }
    if (activeWorkspaceId && document.workspaceId !== activeWorkspaceId) {
      continue;
    }

    seen.add(documentId);
    recent.push(document);

    if (recent.length >= MAX_RECENT_DOCUMENTS) {
      break;
    }
  }

  return recent;
}

export function buildCommandPaletteItems(
  args: BuildCommandPaletteItemsArgs,
): CommandPaletteItem[] {
  const workspaceDocuments = args.activeWorkspaceId
    ? args.documents.filter(
        (document) => document.workspaceId === args.activeWorkspaceId,
      )
    : args.documents;

  const recent = recentDocuments(
    workspaceDocuments,
    args.openDocumentIds,
    args.activeWorkspaceId,
  );

  const recentItems = recent.map((document) => ({
    action: "navigate" as const,
    id: `recent:${document.id}`,
    kind: "recent" as const,
    route: documentRoute(document),
    subtitle: `Recent in ${document.workspaceId}`,
    title: document.path,
  }));

  const fileItems = [...workspaceDocuments]
    .sort((left, right) => left.path.localeCompare(right.path))
    .map((document) => ({
      action: "navigate" as const,
      id: `file:${document.id}`,
      kind: "file" as const,
      route: documentRoute(document),
      subtitle: `File in ${document.workspaceId}`,
      title: document.path,
    }));

  const commandItems: CommandPaletteItem[] = [
    {
      action: "navigate",
      id: "command:settings",
      kind: "command",
      route: "/settings",
      subtitle: "Open app settings",
      title: "Open Settings",
    },
    {
      action: "create-workspace",
      id: "command:create-workspace",
      kind: "command",
      route: null,
      subtitle: "Create and switch to a new workspace",
      title: "Create Workspace",
    },
    ...args.workspaces.map((workspace) => ({
      action: "navigate" as const,
      id: `command:workspace:${workspace.id}`,
      kind: "command" as const,
      route: workspaceRoute(workspace.id),
      subtitle: "Switch workspace",
      title: `Go to ${workspace.name}`,
    })),
  ];

  if (args.activeWorkspaceId) {
    commandItems.push({
      action: "navigate",
      id: "command:active-workspace",
      kind: "command",
      route: workspaceRoute(args.activeWorkspaceId),
      subtitle: "Jump to current workspace",
      title: "Open Active Workspace",
    });
  }

  return [...recentItems, ...fileItems, ...commandItems];
}

export function filterCommandPaletteItems(
  items: CommandPaletteItem[],
  query: string,
): CommandPaletteItem[] {
  const tokens = normalizeQuery(query);
  if (tokens.length === 0) {
    return items;
  }

  return items.filter((item) => {
    const searchable = itemSearchText(item);
    return tokens.every((token) => searchable.includes(token));
  });
}

export function nextPaletteIndex(
  currentIndex: number,
  direction: "up" | "down",
  itemCount: number,
): number {
  if (itemCount <= 0) {
    return -1;
  }

  if (direction === "down") {
    if (currentIndex < 0) {
      return 0;
    }
    return (currentIndex + 1) % itemCount;
  }

  if (currentIndex < 0) {
    return itemCount - 1;
  }
  return currentIndex === 0 ? itemCount - 1 : currentIndex - 1;
}

function kindLabel(kind: CommandPaletteItemKind): string {
  if (kind === "recent") {
    return "Recent";
  }
  if (kind === "file") {
    return "File";
  }
  return "Command";
}

export function CommandPalette({
  activeWorkspaceId,
  documents,
  onCreateWorkspace,
  openDocumentIds,
  workspaces,
}: CommandPaletteProps) {
  const navigate = useNavigate();
  const [isOpen, setIsOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(-1);

  const items = useMemo(
    () =>
      buildCommandPaletteItems({
        activeWorkspaceId,
        documents,
        openDocumentIds,
        workspaces,
      }),
    [activeWorkspaceId, documents, openDocumentIds, workspaces],
  );

  const filteredItems = useMemo(
    () => filterCommandPaletteItems(items, query),
    [items, query],
  );

  useEffect(() => {
    setActiveIndex(filteredItems.length > 0 ? 0 : -1);
  }, [isOpen, filteredItems.length]);

  const closePalette = useCallback(() => {
    setIsOpen(false);
    setQuery("");
    setActiveIndex(-1);
  }, []);

  const runItem = useCallback(
    (item: CommandPaletteItem) => {
      if (item.action === "create-workspace") {
        onCreateWorkspace();
      } else if (item.route) {
        navigate(item.route);
      }

      closePalette();
    },
    [closePalette, navigate, onCreateWorkspace],
  );

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const isPaletteShortcut =
        (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k";

      if (isPaletteShortcut) {
        event.preventDefault();
        setIsOpen((previous) => !previous);
        return;
      }

      if (!isOpen) {
        return;
      }

      if (event.key === "Escape") {
        event.preventDefault();
        closePalette();
        return;
      }

      if (event.key === "ArrowDown") {
        event.preventDefault();
        setActiveIndex((previous) =>
          nextPaletteIndex(previous, "down", filteredItems.length),
        );
        return;
      }

      if (event.key === "ArrowUp") {
        event.preventDefault();
        setActiveIndex((previous) =>
          nextPaletteIndex(previous, "up", filteredItems.length),
        );
        return;
      }

      if (event.key === "Enter" && activeIndex >= 0) {
        event.preventDefault();
        const item = filteredItems[activeIndex];
        if (item) {
          runItem(item);
        }
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [activeIndex, closePalette, filteredItems, isOpen, runItem]);

  return (
    <section
      aria-label="Command palette section"
      data-testid="command-palette-section"
    >
      <button
        aria-expanded={isOpen}
        aria-label="Open command palette"
        data-testid="command-palette-trigger"
        onClick={() => setIsOpen((previous) => !previous)}
        style={{
          alignItems: "center",
          border: "1px solid #d1d5db",
          borderRadius: "6px",
          cursor: "pointer",
          display: "flex",
          fontSize: "0.875rem",
          justifyContent: "space-between",
          marginBottom: "1rem",
          padding: "0.5rem 0.625rem",
          width: "100%",
        }}
        type="button"
      >
        <span>Search files, commands, recent docs</span>
        <span
          aria-hidden="true"
          style={{ color: "#6b7280", fontFamily: "monospace" }}
        >
          Cmd+K
        </span>
      </button>

      {isOpen && (
        <div
          aria-label="Command palette"
          data-testid="command-palette"
          onMouseDown={closePalette}
          style={{
            alignItems: "flex-start",
            background: "rgba(0, 0, 0, 0.28)",
            display: "flex",
            inset: 0,
            justifyContent: "center",
            paddingTop: "10vh",
            position: "fixed",
            zIndex: 500,
          }}
        >
          <div
            onMouseDown={(event) => event.stopPropagation()}
            style={{
              background: "#ffffff",
              border: "1px solid #d1d5db",
              borderRadius: "8px",
              boxShadow: "0 10px 40px rgba(0, 0, 0, 0.2)",
              maxHeight: "70vh",
              overflow: "hidden",
              width: "min(42rem, 92vw)",
            }}
          >
            <label
              htmlFor="command-palette-input"
              style={{
                borderBottom: "1px solid #e5e7eb",
                display: "block",
                fontSize: "0.75rem",
                fontWeight: 600,
                letterSpacing: "0.04em",
                padding: "0.625rem 0.875rem 0.25rem",
                textTransform: "uppercase",
              }}
            >
              Command Palette
            </label>
            <input
              autoFocus
              data-testid="command-palette-input"
              id="command-palette-input"
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Type to search"
              style={{
                border: "none",
                fontSize: "1rem",
                outline: "none",
                padding: "0.75rem 0.875rem",
                width: "100%",
              }}
              type="text"
              value={query}
            />
            <ul
              data-testid="command-palette-results"
              role="listbox"
              style={{
                listStyle: "none",
                margin: 0,
                maxHeight: "50vh",
                overflowY: "auto",
                padding: "0.25rem",
              }}
            >
              {filteredItems.length === 0 && (
                <li
                  data-testid="command-palette-empty"
                  style={{ color: "#6b7280", padding: "0.75rem" }}
                >
                  No matches found.
                </li>
              )}
              {filteredItems.map((item, index) => (
                <li key={item.id} role="option">
                  <button
                    aria-selected={index === activeIndex}
                    data-testid={`command-palette-item-${item.id}`}
                    onClick={() => runItem(item)}
                    style={{
                      background:
                        index === activeIndex ? "#eff6ff" : "transparent",
                      border: "none",
                      borderRadius: "4px",
                      cursor: "pointer",
                      display: "block",
                      padding: "0.5rem 0.625rem",
                      textAlign: "left",
                      width: "100%",
                    }}
                    type="button"
                  >
                    <div style={{ fontSize: "0.875rem", fontWeight: 600 }}>
                      {item.title}
                    </div>
                    <div style={{ color: "#6b7280", fontSize: "0.75rem" }}>
                      {kindLabel(item.kind)} | {item.subtitle}
                    </div>
                  </button>
                </li>
              ))}
            </ul>
          </div>
        </div>
      )}
    </section>
  );
}
