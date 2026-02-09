import { Menu } from "@base-ui-components/react/menu";
import type { Document, Workspace } from "@scriptum/shared";
import clsx from "clsx";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import styles from "./CommandPalette.module.css";

const MAX_RECENT_DOCUMENTS = 5;

export type CommandPaletteItemKind = "command" | "file" | "recent";
export type CommandPaletteAction =
  | "create-workspace"
  | "navigate"
  | "new-document"
  | "open-search";

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
  onCreateDocument?: () => void;
  onCreateWorkspace: () => void;
  onOpenSearchPanel?: () => void;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
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
      action: "new-document",
      id: "command:new-document",
      kind: "command",
      route: null,
      subtitle: "Shortcut: Cmd+N",
      title: "New Document",
    },
    {
      action: "open-search",
      id: "command:open-search",
      kind: "command",
      route: null,
      subtitle: "Shortcut: Cmd+Shift+F",
      title: "Open Search Panel",
    },
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
  onCreateDocument = () => undefined,
  onCreateWorkspace,
  onOpenSearchPanel = () => undefined,
  open,
  onOpenChange,
  openDocumentIds,
  workspaces,
}: CommandPaletteProps) {
  const navigate = useNavigate();
  const [uncontrolledOpen, setUncontrolledOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(-1);
  const isOpen = open ?? uncontrolledOpen;

  const setIsOpen = useCallback(
    (nextOpen: boolean) => {
      onOpenChange?.(nextOpen);
      if (open === undefined) {
        setUncontrolledOpen(nextOpen);
      }
    },
    [onOpenChange, open],
  );

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

  const highlightedIndex = useMemo(() => {
    if (!isOpen || filteredItems.length === 0) {
      return -1;
    }

    if (activeIndex < 0) {
      return 0;
    }

    if (activeIndex >= filteredItems.length) {
      return filteredItems.length - 1;
    }

    return activeIndex;
  }, [activeIndex, filteredItems.length, isOpen]);

  const closePalette = useCallback(() => {
    setIsOpen(false);
    setQuery("");
    setActiveIndex(-1);
  }, [setIsOpen]);

  const runItem = useCallback(
    (item: CommandPaletteItem) => {
      if (item.action === "create-workspace") {
        onCreateWorkspace();
      } else if (item.action === "new-document") {
        onCreateDocument();
      } else if (item.action === "open-search") {
        onOpenSearchPanel();
      } else if (item.route) {
        navigate(item.route);
      }

      closePalette();
    },
    [
      closePalette,
      navigate,
      onCreateDocument,
      onCreateWorkspace,
      onOpenSearchPanel,
    ],
  );

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const isPaletteShortcut =
        (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k";

      if (isPaletteShortcut) {
        event.preventDefault();
        setIsOpen(!isOpen);
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
        setActiveIndex(
          nextPaletteIndex(highlightedIndex, "down", filteredItems.length),
        );
        return;
      }

      if (event.key === "ArrowUp") {
        event.preventDefault();
        setActiveIndex(
          nextPaletteIndex(highlightedIndex, "up", filteredItems.length),
        );
        return;
      }

      if (event.key === "Enter" && highlightedIndex >= 0) {
        event.preventDefault();
        const item = filteredItems[highlightedIndex];
        if (item) {
          runItem(item);
        }
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    closePalette,
    filteredItems,
    highlightedIndex,
    isOpen,
    runItem,
    setIsOpen,
  ]);

  return (
    <section
      aria-label="Command palette section"
      className={styles.section}
      data-testid="command-palette-section"
    >
      <Menu.Root
        onOpenChange={(open) => {
          if (!open) {
            closePalette();
          }
        }}
        open={isOpen}
      >
        <Menu.Trigger
          aria-expanded={isOpen}
          aria-label="Open command palette"
          className={styles.trigger}
          data-testid="command-palette-trigger"
          onClick={() => setIsOpen(!isOpen)}
          type="button"
        >
          <span>Search files, commands, recent docs</span>
          <span aria-hidden="true" className={styles.triggerShortcut}>
            Cmd+K
          </span>
        </Menu.Trigger>

        <Menu.Portal>
          <Menu.Backdrop
            aria-label="Command palette"
            className={styles.overlay}
            data-motion={isOpen ? "enter" : "exit"}
            data-testid={isOpen ? "command-palette" : undefined}
            onMouseDown={closePalette}
          >
            <Menu.Positioner>
              <Menu.Popup
                className={styles.panel}
                data-motion={isOpen ? "enter" : "exit"}
                onMouseDown={(event) => event.stopPropagation()}
              >
                <label
                  className={styles.panelLabel}
                  htmlFor="command-palette-input"
                >
                  Command Palette
                </label>
                <input
                  className={styles.panelInput}
                  data-testid="command-palette-input"
                  id="command-palette-input"
                  onChange={(event) => setQuery(event.target.value)}
                  placeholder="Type to search"
                  type="text"
                  value={query}
                />
                <ul
                  className={styles.results}
                  data-testid="command-palette-results"
                >
                  {filteredItems.length === 0 ? (
                    <li
                      className={styles.empty}
                      data-testid="command-palette-empty"
                    >
                      No matches found.
                    </li>
                  ) : null}
                  {filteredItems.map((item, index) => (
                    <li key={item.id}>
                      <Menu.Item
                        aria-selected={index === highlightedIndex}
                        className={clsx(
                          styles.itemButton,
                          index === highlightedIndex && styles.itemButtonActive,
                        )}
                        data-testid={`command-palette-item-${item.id}`}
                        onClick={() => runItem(item)}
                      >
                        <div className={styles.itemTitle}>{item.title}</div>
                        <div className={styles.itemSubtitle}>
                          {kindLabel(item.kind)} | {item.subtitle}
                        </div>
                      </Menu.Item>
                    </li>
                  ))}
                </ul>
              </Menu.Popup>
            </Menu.Positioner>
          </Menu.Backdrop>
        </Menu.Portal>
      </Menu.Root>
    </section>
  );
}
