import type { Document } from "@scriptum/shared";
import {
  type ChangeEvent,
  type MouseEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

// -- Types --------------------------------------------------------------------

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

export type ContextMenuAction =
  | "new-folder"
  | "rename"
  | "move"
  | "delete"
  | "copy-link"
  | "add-tag"
  | "archive";

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
  /** Called when user commits an inline rename. */
  onRenameDocument?: (documentId: string, nextPath: string) => void;
  /** Newly-created document id that should enter inline rename mode. */
  pendingRenameDocumentId?: string | null;
}

// -- Tree building -------------------------------------------------------------

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

    for (let index = 0; index < segments.length; index += 1) {
      const segment = segments[index];
      const isFile = index === segments.length - 1;
      const fullPath = segments.slice(0, index + 1).join("/");

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
  nodes.sort((left, right) => {
    const leftIsFolder = left.children.length > 0 && !left.document;
    const rightIsFolder = right.children.length > 0 && !right.document;
    if (leftIsFolder && !rightIsFolder) return -1;
    if (!leftIsFolder && rightIsFolder) return 1;
    return left.name.localeCompare(right.name);
  });

  for (const node of nodes) {
    if (node.children.length > 0) {
      sortTreeNodes(node.children);
    }
  }
}

// -- Icon helpers --------------------------------------------------------------

export function fileIcon(name: string): string {
  if (name.endsWith(".md") || name.endsWith(".markdown")) return "\u{1F4DD}";
  if (name.endsWith(".json")) return "\u{1F4CB}";
  if (name.endsWith(".yaml") || name.endsWith(".yml")) return "\u{2699}";
  if (name.endsWith(".toml")) return "\u{2699}";
  return "\u{1F4C4}";
}

// -- Context menu --------------------------------------------------------------

const CONTEXT_ACTIONS: { action: ContextMenuAction; label: string }[] = [
  { action: "new-folder", label: "New Folder" },
  { action: "rename", label: "Rename" },
  { action: "move", label: "Move" },
  { action: "delete", label: "Delete" },
  { action: "copy-link", label: "Copy Link" },
  { action: "add-tag", label: "Add Tag" },
  { action: "archive", label: "Archive" },
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

// -- Tree node component -------------------------------------------------------

function TreeNodeItem({
  activeDocumentId,
  depth,
  editingDocumentId,
  editingPath,
  expanded,
  node,
  onContextMenu,
  onDocumentSelect,
  onRenameCancel,
  onRenameChange,
  onRenameCommit,
  onToggle,
}: {
  activeDocumentId: string | null;
  depth: number;
  editingDocumentId: string | null;
  editingPath: string;
  expanded: Set<string>;
  node: TreeNode;
  onContextMenu: (event: MouseEvent, doc: Document) => void;
  onDocumentSelect?: (documentId: string) => void;
  onRenameCancel: () => void;
  onRenameChange: (value: string) => void;
  onRenameCommit: (documentId: string) => void;
  onToggle: (path: string) => void;
}) {
  const isFolder = node.children.length > 0;
  const isExpanded = expanded.has(node.fullPath);
  const isActive = node.document?.id === activeDocumentId;
  const isEditing = node.document?.id === editingDocumentId;

  const handleClick = () => {
    if (isFolder) {
      onToggle(node.fullPath);
    } else if (node.document && onDocumentSelect) {
      onDocumentSelect(node.document.id);
    }
  };

  const handleContextMenu = (event: MouseEvent) => {
    if (!node.document) {
      return;
    }
    event.preventDefault();
    onContextMenu(event, node.document);
  };

  const handleRenameInputKeyDown = (
    event: ReactKeyboardEvent<HTMLInputElement>,
  ) => {
    if (!node.document) {
      return;
    }

    if (event.key === "Enter") {
      event.preventDefault();
      onRenameCommit(node.document.id);
      return;
    }

    if (event.key === "Escape") {
      event.preventDefault();
      onRenameCancel();
    }
  };

  if (isEditing && node.document) {
    return (
      <li data-testid={`tree-node-${node.fullPath}`} role="treeitem">
        <div
          style={{
            alignItems: "center",
            display: "flex",
            gap: "0.35rem",
            paddingBottom: "2px",
            paddingLeft: `${depth * 16 + 4}px`,
            paddingRight: "4px",
            paddingTop: "2px",
          }}
        >
          <span aria-hidden="true">{fileIcon(node.name)}</span>
          <input
            autoFocus
            data-testid={`tree-rename-input-${node.document.id}`}
            onBlur={() => onRenameCommit(node.document!.id)}
            onChange={(event: ChangeEvent<HTMLInputElement>) =>
              onRenameChange(event.target.value)
            }
            onKeyDown={handleRenameInputKeyDown}
            style={{ flex: 1, minWidth: 0 }}
            type="text"
            value={editingPath}
          />
        </div>
      </li>
    );
  }

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
                editingDocumentId={editingDocumentId}
                editingPath={editingPath}
                expanded={expanded}
                key={child.fullPath}
                node={child}
                onContextMenu={onContextMenu}
                onDocumentSelect={onDocumentSelect}
                onRenameCancel={onRenameCancel}
                onRenameChange={onRenameChange}
                onRenameCommit={onRenameCommit}
                onToggle={onToggle}
              />
            ))}
          </ul>
        </li>
      )}
    </>
  );
}

// -- Main component ------------------------------------------------------------

export function DocumentTree({
  documents,
  activeDocumentId,
  onDocumentSelect,
  onContextMenuAction,
  onRenameDocument,
  pendingRenameDocumentId = null,
}: DocumentTreeProps) {
  const tree = useMemo(() => buildTree(documents), [documents]);
  const [expanded, setExpanded] = useState<Set<string>>(() => {
    return new Set(
      tree
        .filter((node) => node.children.length > 0)
        .map((node) => node.fullPath),
    );
  });
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [editingDocumentId, setEditingDocumentId] = useState<string | null>(
    null,
  );
  const [editingPath, setEditingPath] = useState("");
  const consumedPendingRenameIdRef = useRef<string | null>(null);

  useEffect(() => {
    if (!pendingRenameDocumentId) {
      consumedPendingRenameIdRef.current = null;
      return;
    }
    if (consumedPendingRenameIdRef.current === pendingRenameDocumentId) {
      return;
    }

    const pendingDocument = documents.find(
      (document) => document.id === pendingRenameDocumentId,
    );
    if (!pendingDocument) {
      return;
    }

    consumedPendingRenameIdRef.current = pendingRenameDocumentId;
    setEditingDocumentId(pendingDocument.id);
    setEditingPath(pendingDocument.path);
  }, [documents, pendingRenameDocumentId]);

  const handleToggle = useCallback((path: string) => {
    setExpanded((previous) => {
      const next = new Set(previous);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }, []);

  const handleContextMenu = useCallback(
    (event: MouseEvent, document: Document) => {
      setContextMenu({ document, x: event.clientX, y: event.clientY });
    },
    [],
  );

  const closeContextMenu = useCallback(() => setContextMenu(null), []);

  const beginRenameDocument = useCallback((document: Document) => {
    setEditingDocumentId(document.id);
    setEditingPath(document.path);
  }, []);

  const cancelRenameDocument = useCallback(() => {
    setEditingDocumentId(null);
    setEditingPath("");
  }, []);

  const commitRenameDocument = useCallback(
    (documentId: string) => {
      const nextPath = editingPath.trim();
      if (!nextPath) {
        cancelRenameDocument();
        return;
      }

      onRenameDocument?.(documentId, nextPath);
      setEditingDocumentId(null);
      setEditingPath("");
    },
    [cancelRenameDocument, editingPath, onRenameDocument],
  );

  const handleContextAction = useCallback(
    (action: ContextMenuAction, document: Document) => {
      if (action === "rename") {
        beginRenameDocument(document);
        return;
      }
      onContextMenuAction?.(action, document);
    },
    [beginRenameDocument, onContextMenuAction],
  );

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
            editingDocumentId={editingDocumentId}
            editingPath={editingPath}
            expanded={expanded}
            key={node.fullPath}
            node={node}
            onContextMenu={handleContextMenu}
            onDocumentSelect={onDocumentSelect}
            onRenameCancel={cancelRenameDocument}
            onRenameChange={setEditingPath}
            onRenameCommit={commitRenameDocument}
            onToggle={handleToggle}
          />
        ))}
      </ul>
      {contextMenu ? (
        <ContextMenu
          menu={contextMenu}
          onAction={handleContextAction}
          onClose={closeContextMenu}
        />
      ) : null}
    </nav>
  );
}
