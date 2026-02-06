import type { Document } from "@scriptum/shared";
import { type MouseEvent, useCallback, useMemo, useState } from "react";

// ── Types ────────────────────────────────────────────────────────────────────

export interface TreeNode {
  /** Segment name (folder name or file basename). */
  name: string;
  /** Full path from root. */
  fullPath: string;
  /** Document, if this node is a file (leaf). */
  document: Document | null;
  /** Sorted child nodes. */
  children: TreeNode[];
}

export type ContextMenuAction = "rename" | "move" | "delete" | "add-tag";

export interface ContextMenuState {
  x: number;
  y: number;
  document: Document;
}

export interface DocumentTreeProps {
  /** Documents to display in the tree. */
  documents: Document[];
  /** ID of the currently active document. */
  activeDocumentId: string | null;
  /** Called when user clicks a document node. */
  onDocumentSelect?: (documentId: string) => void;
  /** Called when user picks a context menu action. */
  onContextMenuAction?: (action: ContextMenuAction, document: Document) => void;
}

// ── Tree building ────────────────────────────────────────────────────────────

export function buildTree(documents: Document[]): TreeNode[] {
  const root: TreeNode = {
    name: "",
    fullPath: "",
    document: null,
    children: [],
  };

  for (const doc of documents) {
    const segments = doc.path.split("/").filter(Boolean);
    let current = root;

    for (let i = 0; i < segments.length; i++) {
      const segment = segments[i];
      const isFile = i === segments.length - 1;
      const fullPath = segments.slice(0, i + 1).join("/");

      let child = current.children.find((node) => node.name === segment);
      if (!child) {
        child = {
          name: segment,
          fullPath,
          document: isFile ? doc : null,
          children: [],
        };
        current.children.push(child);
      } else if (isFile && !child.document) {
        child.document = doc;
      }

      current = child;
    }
  }

  sortTreeNodes(root.children);
  return root.children;
}

function sortTreeNodes(nodes: TreeNode[]): void {
  nodes.sort((a, b) => {
    // Folders first, then files.
    const aIsFolder = a.children.length > 0 && !a.document;
    const bIsFolder = b.children.length > 0 && !b.document;
    if (aIsFolder && !bIsFolder) return -1;
    if (!aIsFolder && bIsFolder) return 1;
    return a.name.localeCompare(b.name);
  });

  for (const node of nodes) {
    if (node.children.length > 0) {
      sortTreeNodes(node.children);
    }
  }
}

// ── Icon helpers ─────────────────────────────────────────────────────────────

export function fileIcon(name: string): string {
  if (name.endsWith(".md") || name.endsWith(".markdown")) return "\u{1F4DD}";
  if (name.endsWith(".json")) return "\u{1F4CB}";
  if (name.endsWith(".yaml") || name.endsWith(".yml")) return "\u{2699}";
  if (name.endsWith(".toml")) return "\u{2699}";
  return "\u{1F4C4}";
}

// ── Context menu ─────────────────────────────────────────────────────────────

const CONTEXT_ACTIONS: { action: ContextMenuAction; label: string }[] = [
  { action: "rename", label: "Rename" },
  { action: "move", label: "Move" },
  { action: "delete", label: "Delete" },
  { action: "add-tag", label: "Add Tag" },
];

function ContextMenu({
  menu,
  onAction,
  onClose,
}: {
  menu: ContextMenuState;
  onAction: (action: ContextMenuAction, doc: Document) => void;
  onClose: () => void;
}) {
  return (
    <ul
      data-testid="context-menu"
      role="menu"
      style={{
        background: "#fff",
        border: "1px solid #d1d5db",
        borderRadius: "4px",
        boxShadow: "0 2px 8px rgba(0,0,0,0.15)",
        left: menu.x,
        listStyle: "none",
        margin: 0,
        padding: "4px 0",
        position: "fixed",
        top: menu.y,
        zIndex: 1000,
      }}
    >
      {CONTEXT_ACTIONS.map(({ action, label }) => (
        <li key={action}>
          <button
            data-testid={`context-action-${action}`}
            onClick={() => {
              onAction(action, menu.document);
              onClose();
            }}
            role="menuitem"
            style={{
              background: "none",
              border: "none",
              cursor: "pointer",
              display: "block",
              padding: "6px 16px",
              textAlign: "left",
              width: "100%",
            }}
            type="button"
          >
            {label}
          </button>
        </li>
      ))}
    </ul>
  );
}

// ── Tree node component ──────────────────────────────────────────────────────

function TreeNodeItem({
  activeDocumentId,
  depth,
  expanded,
  node,
  onContextMenu,
  onDocumentSelect,
  onToggle,
}: {
  activeDocumentId: string | null;
  depth: number;
  expanded: Set<string>;
  node: TreeNode;
  onContextMenu: (event: MouseEvent, doc: Document) => void;
  onDocumentSelect?: (documentId: string) => void;
  onToggle: (path: string) => void;
}) {
  const isFolder = node.children.length > 0;
  const isExpanded = expanded.has(node.fullPath);
  const isActive = node.document?.id === activeDocumentId;

  const handleClick = () => {
    if (isFolder) {
      onToggle(node.fullPath);
    } else if (node.document && onDocumentSelect) {
      onDocumentSelect(node.document.id);
    }
  };

  const handleContextMenu = (event: MouseEvent) => {
    if (node.document) {
      event.preventDefault();
      onContextMenu(event, node.document);
    }
  };

  return (
    <>
      <li
        aria-expanded={isFolder ? isExpanded : undefined}
        data-active={isActive || undefined}
        data-testid={`tree-node-${node.fullPath}`}
        role="treeitem"
      >
        <button
          aria-label={node.name}
          onClick={handleClick}
          onContextMenu={handleContextMenu}
          style={{
            background: isActive ? "#e0f2fe" : "none",
            border: "none",
            cursor: "pointer",
            display: "block",
            fontWeight: isActive ? 600 : 400,
            paddingBottom: "2px",
            paddingLeft: `${depth * 16 + 4}px`,
            paddingRight: "4px",
            paddingTop: "2px",
            textAlign: "left",
            width: "100%",
          }}
          type="button"
        >
          <span aria-hidden="true" style={{ marginRight: "4px" }}>
            {isFolder
              ? isExpanded
                ? "\u{1F4C2}"
                : "\u{1F4C1}"
              : fileIcon(node.name)}
          </span>
          {node.name}
        </button>
      </li>
      {isFolder && isExpanded && (
        <li role="none">
          <ul role="group" style={{ listStyle: "none", margin: 0, padding: 0 }}>
            {node.children.map((child) => (
              <TreeNodeItem
                activeDocumentId={activeDocumentId}
                depth={depth + 1}
                expanded={expanded}
                key={child.fullPath}
                node={child}
                onContextMenu={onContextMenu}
                onDocumentSelect={onDocumentSelect}
                onToggle={onToggle}
              />
            ))}
          </ul>
        </li>
      )}
    </>
  );
}

// ── Main component ───────────────────────────────────────────────────────────

export function DocumentTree({
  documents,
  activeDocumentId,
  onDocumentSelect,
  onContextMenuAction,
}: DocumentTreeProps) {
  const tree = useMemo(() => buildTree(documents), [documents]);
  const [expanded, setExpanded] = useState<Set<string>>(() => {
    // Auto-expand top-level folders.
    return new Set(
      tree.filter((n) => n.children.length > 0).map((n) => n.fullPath)
    );
  });
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);

  const handleToggle = useCallback((path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }, []);

  const handleContextMenu = useCallback(
    (event: MouseEvent, doc: Document) => {
      setContextMenu({ x: event.clientX, y: event.clientY, document: doc });
    },
    []
  );

  const handleContextAction = useCallback(
    (action: ContextMenuAction, doc: Document) => {
      onContextMenuAction?.(action, doc);
    },
    [onContextMenuAction]
  );

  const closeContextMenu = useCallback(() => setContextMenu(null), []);

  if (documents.length === 0) {
    return (
      <div data-testid="document-tree-empty">
        <p style={{ color: "#6b7280", fontSize: "0.875rem" }}>
          No documents yet.
        </p>
      </div>
    );
  }

  return (
    <nav aria-label="Document tree" data-testid="document-tree">
      <ul
        onClick={closeContextMenu}
        role="tree"
        style={{ listStyle: "none", margin: 0, padding: 0 }}
      >
        {tree.map((node) => (
          <TreeNodeItem
            activeDocumentId={activeDocumentId}
            depth={0}
            expanded={expanded}
            key={node.fullPath}
            node={node}
            onContextMenu={handleContextMenu}
            onDocumentSelect={onDocumentSelect}
            onToggle={handleToggle}
          />
        ))}
      </ul>
      {contextMenu && (
        <ContextMenu
          menu={contextMenu}
          onAction={handleContextAction}
          onClose={closeContextMenu}
        />
      )}
    </nav>
  );
}
