import { ContextMenu } from "@base-ui-components/react/context-menu";
import type { Document } from "@scriptum/shared";
import clsx from "clsx";
import {
  type ChangeEvent,
  type DragEvent as ReactDragEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import controls from "../../styles/Controls.module.css";
import { SkeletonStack } from "../Skeleton";
import styles from "./DocumentTree.module.css";

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
  | "archive"
  | "unarchive";

export interface DocumentTreeProps {
  /** Documents to display in the tree. */
  documents: Document[];
  /** Whether the tree is waiting on initial data. */
  loading?: boolean;
  /** ID of the currently active document. */
  activeDocumentId: string | null;
  /** Called when user clicks a document node. */
  onDocumentSelect?: (documentId: string) => void;
  /** Called when a folder is selected. */
  onFolderSelect?: (folderPath: string) => void;
  /** Called when user picks a context menu action. */
  onContextMenuAction?: (action: ContextMenuAction, document: Document) => void;
  /** Called when user commits an inline rename. */
  onRenameDocument?: (documentId: string, nextPath: string) => void;
  /** Newly-created document id that should enter inline rename mode. */
  pendingRenameDocumentId?: string | null;
  /** Selected folder path for folder picker mode. */
  selectedFolderPath?: string | null;
}

const DOCUMENT_TREE_LOADING_LINE_CLASSNAMES = [
  clsx(styles.loadingLine, styles.loading62),
  clsx(styles.loadingLine, styles.loading78),
  clsx(styles.loadingLine, styles.loading54),
  clsx(styles.loadingLine, styles.loading71),
  clsx(styles.loadingLine, styles.loading49),
];

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

function parentPath(path: string): string {
  const lastSlash = path.lastIndexOf("/");
  if (lastSlash < 0) {
    return "";
  }
  return path.slice(0, lastSlash);
}

function baseName(path: string): string {
  const segments = path.split("/").filter(Boolean);
  return segments[segments.length - 1] ?? path;
}

function applyPreferredOrder(
  nodes: TreeNode[],
  preferredOrderByParent: Record<string, string[]>,
  parent = "",
): TreeNode[] {
  const preferredOrder = preferredOrderByParent[parent];
  const preferredIndex = new Map<string, number>();
  if (preferredOrder) {
    for (const [index, path] of preferredOrder.entries()) {
      preferredIndex.set(path, index);
    }
  }

  const ordered = nodes
    .map((node) => ({
      ...node,
      children: applyPreferredOrder(
        node.children,
        preferredOrderByParent,
        node.fullPath,
      ),
    }))
    .sort((left, right) => {
      const leftIndex = preferredIndex.get(left.fullPath);
      const rightIndex = preferredIndex.get(right.fullPath);
      if (leftIndex !== undefined && rightIndex !== undefined) {
        return leftIndex - rightIndex;
      }
      if (leftIndex !== undefined) {
        return -1;
      }
      if (rightIndex !== undefined) {
        return 1;
      }
      return 0;
    });

  return ordered;
}

function findNodeByPath(nodes: TreeNode[], path: string): TreeNode | null {
  for (const node of nodes) {
    if (node.fullPath === path) {
      return node;
    }
    const inChildren = findNodeByPath(node.children, path);
    if (inChildren) {
      return inChildren;
    }
  }
  return null;
}

function findSiblingPaths(nodes: TreeNode[], parent: string): string[] | null {
  if (parent.length === 0) {
    return nodes.map((node) => node.fullPath);
  }

  for (const node of nodes) {
    if (node.fullPath === parent) {
      return node.children.map((child) => child.fullPath);
    }

    const fromChildren = findSiblingPaths(node.children, parent);
    if (fromChildren) {
      return fromChildren;
    }
  }

  return null;
}

function flattenVisibleTreeNodes(
  nodes: TreeNode[],
  expanded: Set<string>,
): TreeNode[] {
  const visible: TreeNode[] = [];

  const walk = (treeNodes: TreeNode[]) => {
    for (const node of treeNodes) {
      visible.push(node);
      if (node.children.length > 0 && expanded.has(node.fullPath)) {
        walk(node.children);
      }
    }
  };

  walk(nodes);
  return visible;
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

const BASE_CONTEXT_ACTIONS: { action: ContextMenuAction; label: string }[] = [
  { action: "new-folder", label: "New Folder" },
  { action: "rename", label: "Rename" },
  { action: "move", label: "Move" },
  { action: "delete", label: "Delete" },
  { action: "copy-link", label: "Copy Link" },
  { action: "add-tag", label: "Add Tag" },
];

function contextActionsForDocument(
  document: Document,
): { action: ContextMenuAction; label: string }[] {
  return [
    ...BASE_CONTEXT_ACTIONS,
    document.archivedAt
      ? { action: "unarchive", label: "Unarchive" }
      : { action: "archive", label: "Archive" },
  ];
}

// -- Tree node component -------------------------------------------------------

function TreeNodeItem({
  activeDocumentId,
  draggingDocumentPath,
  dropTargetPath,
  editingDocumentId,
  editingPath,
  expanded,
  focusedPath,
  node,
  onContextAction,
  onDragEndDocument,
  onDragEnterTarget,
  onDragStartDocument,
  onDocumentSelect,
  onFolderSelect,
  onDropOnFile,
  onDropOnFolder,
  onFocusPath,
  onRenameCancel,
  onRenameChange,
  onRenameCommit,
  onToggle,
  selectedFolderPath,
}: {
  activeDocumentId: string | null;
  draggingDocumentPath: string | null;
  dropTargetPath: string | null;
  editingDocumentId: string | null;
  editingPath: string;
  expanded: Set<string>;
  focusedPath: string | null;
  node: TreeNode;
  onContextAction: (action: ContextMenuAction, doc: Document) => void;
  onDragEndDocument: () => void;
  onDragEnterTarget: (path: string) => void;
  onDragStartDocument: (node: TreeNode) => void;
  onDocumentSelect?: (documentId: string) => void;
  onFolderSelect?: (folderPath: string) => void;
  onDropOnFile: (node: TreeNode) => void;
  onDropOnFolder: (node: TreeNode) => void;
  onFocusPath: (path: string) => void;
  onRenameCancel: () => void;
  onRenameChange: (value: string) => void;
  onRenameCommit: (documentId: string) => void;
  onToggle: (path: string) => void;
  selectedFolderPath: string | null;
}) {
  const isFolder = node.children.length > 0;
  const isExpanded = expanded.has(node.fullPath);
  const isActive = node.document?.id === activeDocumentId;
  const isEditing = node.document?.id === editingDocumentId;
  const isDropTarget = dropTargetPath === node.fullPath;
  const isFocused = focusedPath === node.fullPath;
  const isArchived = Boolean(node.document?.archivedAt);
  const isSelectedFolder = isFolder && selectedFolderPath === node.fullPath;

  const handleClick = () => {
    onFocusPath(node.fullPath);
    if (isFolder) {
      onFolderSelect?.(node.fullPath);
      onToggle(node.fullPath);
    } else if (node.document && onDocumentSelect) {
      onDocumentSelect(node.document.id);
    }
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

  const handleDragStart = (event: ReactDragEvent<HTMLButtonElement>) => {
    if (!node.document) {
      return;
    }

    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData("text/plain", node.fullPath);
    onDragStartDocument(node);
  };

  const handleDragOver = (event: ReactDragEvent<HTMLButtonElement>) => {
    if (!draggingDocumentPath || draggingDocumentPath === node.fullPath) {
      return;
    }

    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    onDragEnterTarget(node.fullPath);
  };

  const handleDrop = (event: ReactDragEvent<HTMLButtonElement>) => {
    if (!draggingDocumentPath || draggingDocumentPath === node.fullPath) {
      return;
    }

    event.preventDefault();
    if (isFolder) {
      onDropOnFolder(node);
      return;
    }
    if (node.document) {
      onDropOnFile(node);
    }
  };

  if (isEditing && node.document) {
    return (
      <li
        className={styles.treeItem}
        data-testid={`tree-node-${node.fullPath}`}
        data-tree-path={node.fullPath}
        role="treeitem"
        tabIndex={-1}
      >
        <div className={styles.renameRow}>
          <span aria-hidden="true" className={styles.treeIcon}>
            {fileIcon(node.name)}
          </span>
          <input
            aria-label="Rename document"
            className={clsx(controls.textInput, styles.renameInput)}
            data-testid={`tree-rename-input-${node.document.id}`}
            onBlur={() => {
              if (node.document) onRenameCommit(node.document.id);
            }}
            onChange={(event: ChangeEvent<HTMLInputElement>) =>
              onRenameChange(event.target.value)
            }
            onKeyDown={handleRenameInputKeyDown}
            type="text"
            value={editingPath}
          />
        </div>
      </li>
    );
  }

  const treeNodeButton = (
    <button
      aria-label={node.name}
      className={clsx(
        styles.treeNodeButton,
        isActive && styles.treeNodeButtonActive,
        isDropTarget && styles.treeNodeButtonDropTarget,
        isArchived && styles.treeNodeButtonArchived,
        isSelectedFolder && styles.treeNodeButtonSelected,
      )}
      draggable={Boolean(node.document)}
      onClick={handleClick}
      onDragEnd={onDragEndDocument}
      onDragOver={handleDragOver}
      onDragStart={handleDragStart}
      onDrop={handleDrop}
      tabIndex={-1}
      type="button"
    >
      <span aria-hidden="true" className={styles.treeIcon}>
        {isFolder
          ? isExpanded
            ? "\u{1F4C2}"
            : "\u{1F4C1}"
          : fileIcon(node.name)}
      </span>
      <span>{node.name}</span>
      {isArchived ? (
        <span className={styles.archivedBadge}>Archived</span>
      ) : null}
    </button>
  );

  return (
    <>
      <li
        aria-expanded={isFolder ? isExpanded : undefined}
        className={styles.treeItem}
        data-active={isActive || undefined}
        data-archived={isArchived || undefined}
        data-drop-target={isDropTarget || undefined}
        data-folder-selected={isSelectedFolder || undefined}
        data-testid={`tree-node-${node.fullPath}`}
        data-tree-path={node.fullPath}
        onFocus={() => onFocusPath(node.fullPath)}
        role="treeitem"
        tabIndex={isFocused ? 0 : -1}
      >
        {node.document ? (
          <ContextMenu.Root>
            <ContextMenu.Trigger render={treeNodeButton} />
            <ContextMenu.Portal>
              <ContextMenu.Positioner>
                <ContextMenu.Popup
                  className={styles.contextMenu}
                  data-testid="context-menu"
                >
                  {contextActionsForDocument(node.document).map(
                    ({ action, label }) => (
                      <ContextMenu.Item
                        className={styles.contextMenuItem}
                        data-testid={`context-action-${action}`}
                        key={action}
                        onClick={() => {
                          if (node.document)
                            onContextAction(action, node.document);
                        }}
                      >
                        {label}
                      </ContextMenu.Item>
                    ),
                  )}
                </ContextMenu.Popup>
              </ContextMenu.Positioner>
            </ContextMenu.Portal>
          </ContextMenu.Root>
        ) : (
          treeNodeButton
        )}
      </li>
      {isFolder && isExpanded && (
        <li role="none">
          {/* biome-ignore lint/a11y/useSemanticElements: role="group" is correct for ARIA tree pattern */}
          <ul className={styles.treeGroup} role="group">
            {node.children.map((child) => (
              <TreeNodeItem
                activeDocumentId={activeDocumentId}
                draggingDocumentPath={draggingDocumentPath}
                dropTargetPath={dropTargetPath}
                editingDocumentId={editingDocumentId}
                editingPath={editingPath}
                expanded={expanded}
                focusedPath={focusedPath}
                key={child.fullPath}
                node={child}
                onContextAction={onContextAction}
                onDragEndDocument={onDragEndDocument}
                onDragEnterTarget={onDragEnterTarget}
                onDragStartDocument={onDragStartDocument}
                onDocumentSelect={onDocumentSelect}
                onFolderSelect={onFolderSelect}
                onDropOnFile={onDropOnFile}
                onDropOnFolder={onDropOnFolder}
                onFocusPath={onFocusPath}
                onRenameCancel={onRenameCancel}
                onRenameChange={onRenameChange}
                onRenameCommit={onRenameCommit}
                onToggle={onToggle}
                selectedFolderPath={selectedFolderPath}
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
  loading = false,
  activeDocumentId,
  onDocumentSelect,
  onFolderSelect,
  onContextMenuAction,
  onRenameDocument,
  pendingRenameDocumentId = null,
  selectedFolderPath = null,
}: DocumentTreeProps) {
  const tree = useMemo(() => buildTree(documents), [documents]);
  const [preferredOrderByParent, setPreferredOrderByParent] = useState<
    Record<string, string[]>
  >({});
  const [expanded, setExpanded] = useState<Set<string>>(() => {
    return new Set(
      tree
        .filter((node) => node.children.length > 0)
        .map((node) => node.fullPath),
    );
  });
  const [draggingDocumentPath, setDraggingDocumentPath] = useState<
    string | null
  >(null);
  const [dropTargetPath, setDropTargetPath] = useState<string | null>(null);
  const [editingDocumentId, setEditingDocumentId] = useState<string | null>(
    null,
  );
  const [editingPath, setEditingPath] = useState("");
  const consumedPendingRenameIdRef = useRef<string | null>(null);
  const treeRef = useRef<HTMLUListElement | null>(null);
  const orderedTree = useMemo(
    () => applyPreferredOrder(tree, preferredOrderByParent),
    [preferredOrderByParent, tree],
  );
  const visibleTreeNodes = useMemo(
    () => flattenVisibleTreeNodes(orderedTree, expanded),
    [expanded, orderedTree],
  );
  const activeDocumentPath = useMemo(
    () =>
      activeDocumentId
        ? (documents.find((document) => document.id === activeDocumentId)
            ?.path ?? null)
        : null,
    [activeDocumentId, documents],
  );
  const [focusedPath, setFocusedPath] = useState<string | null>(null);

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

  useEffect(() => {
    if (editingDocumentId) {
      return;
    }

    const hasFocusedPath =
      focusedPath !== null &&
      visibleTreeNodes.some((node) => node.fullPath === focusedPath);
    if (hasFocusedPath) {
      return;
    }

    if (
      activeDocumentPath &&
      visibleTreeNodes.some((node) => node.fullPath === activeDocumentPath)
    ) {
      setFocusedPath(activeDocumentPath);
      return;
    }

    setFocusedPath(visibleTreeNodes[0]?.fullPath ?? null);
  }, [activeDocumentPath, editingDocumentId, focusedPath, visibleTreeNodes]);

  useEffect(() => {
    if (editingDocumentId || !focusedPath || !treeRef.current) {
      return;
    }

    const treeItem = Array.from(
      treeRef.current.querySelectorAll<HTMLElement>("[data-tree-path]"),
    ).find((element) => element.getAttribute("data-tree-path") === focusedPath);

    if (!treeItem || treeItem === document.activeElement) {
      return;
    }

    treeItem.focus();
  }, [editingDocumentId, focusedPath]);

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

  const clearDragState = useCallback(() => {
    setDraggingDocumentPath(null);
    setDropTargetPath(null);
  }, []);

  const handleDropOnFile = useCallback(
    (targetNode: TreeNode) => {
      if (!draggingDocumentPath) {
        clearDragState();
        return;
      }

      const sourceParent = parentPath(draggingDocumentPath);
      const targetParent = parentPath(targetNode.fullPath);
      if (sourceParent !== targetParent) {
        clearDragState();
        return;
      }

      const siblingPaths = findSiblingPaths(orderedTree, sourceParent);
      if (!siblingPaths) {
        clearDragState();
        return;
      }

      const withoutSource = siblingPaths.filter(
        (path) => path !== draggingDocumentPath,
      );
      const targetIndex = withoutSource.indexOf(targetNode.fullPath);
      if (targetIndex < 0) {
        clearDragState();
        return;
      }

      withoutSource.splice(targetIndex, 0, draggingDocumentPath);
      setPreferredOrderByParent((previous) => ({
        ...previous,
        [sourceParent]: withoutSource,
      }));
      clearDragState();
    },
    [clearDragState, draggingDocumentPath, orderedTree],
  );

  const handleDropOnFolder = useCallback(
    (folderNode: TreeNode) => {
      if (!draggingDocumentPath) {
        clearDragState();
        return;
      }

      const sourceNode = findNodeByPath(orderedTree, draggingDocumentPath);
      if (!sourceNode?.document) {
        clearDragState();
        return;
      }

      const nextPath = `${folderNode.fullPath}/${baseName(sourceNode.document.path)}`;
      if (nextPath !== sourceNode.document.path) {
        onRenameDocument?.(sourceNode.document.id, nextPath);
      }

      setExpanded((previous) => {
        const next = new Set(previous);
        next.add(folderNode.fullPath);
        return next;
      });
      setPreferredOrderByParent((previous) => {
        const sourceParent = parentPath(draggingDocumentPath);
        if (!previous[sourceParent]) {
          return previous;
        }

        return {
          ...previous,
          [sourceParent]: previous[sourceParent].filter(
            (path) => path !== draggingDocumentPath,
          ),
        };
      });
      clearDragState();
    },
    [clearDragState, draggingDocumentPath, onRenameDocument, orderedTree],
  );

  const handleTreeKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLUListElement>) => {
      if (editingDocumentId || visibleTreeNodes.length === 0) {
        return;
      }

      const currentPath = focusedPath ?? visibleTreeNodes[0]?.fullPath ?? null;
      if (!currentPath) {
        return;
      }

      const currentIndex = visibleTreeNodes.findIndex(
        (node) => node.fullPath === currentPath,
      );
      if (currentIndex < 0) {
        return;
      }

      const currentNode = visibleTreeNodes[currentIndex];
      if (!currentNode) {
        return;
      }

      switch (event.key) {
        case "ArrowDown": {
          event.preventDefault();
          const nextNode =
            visibleTreeNodes[
              Math.min(currentIndex + 1, visibleTreeNodes.length - 1)
            ];
          setFocusedPath(nextNode?.fullPath ?? currentPath);
          return;
        }
        case "ArrowUp": {
          event.preventDefault();
          const previousNode = visibleTreeNodes[Math.max(currentIndex - 1, 0)];
          setFocusedPath(previousNode?.fullPath ?? currentPath);
          return;
        }
        case "Home": {
          event.preventDefault();
          setFocusedPath(visibleTreeNodes[0]?.fullPath ?? currentPath);
          return;
        }
        case "End": {
          event.preventDefault();
          setFocusedPath(
            visibleTreeNodes[visibleTreeNodes.length - 1]?.fullPath ??
              currentPath,
          );
          return;
        }
        case "ArrowRight": {
          if (currentNode.children.length === 0) {
            return;
          }

          event.preventDefault();
          if (!expanded.has(currentNode.fullPath)) {
            setExpanded((previous) => {
              const next = new Set(previous);
              next.add(currentNode.fullPath);
              return next;
            });
            return;
          }

          const firstChild = currentNode.children[0];
          if (firstChild) {
            setFocusedPath(firstChild.fullPath);
          }
          return;
        }
        case "ArrowLeft": {
          if (
            currentNode.children.length > 0 &&
            expanded.has(currentNode.fullPath)
          ) {
            event.preventDefault();
            setExpanded((previous) => {
              const next = new Set(previous);
              next.delete(currentNode.fullPath);
              return next;
            });
            return;
          }

          const parent = parentPath(currentNode.fullPath);
          if (parent.length > 0) {
            event.preventDefault();
            setFocusedPath(parent);
          }
          return;
        }
        case "Enter": {
          event.preventDefault();
          if (currentNode.children.length > 0) {
            handleToggle(currentNode.fullPath);
            return;
          }

          if (currentNode.document && onDocumentSelect) {
            onDocumentSelect(currentNode.document.id);
          }
          return;
        }
        default:
          return;
      }
    },
    [
      editingDocumentId,
      expanded,
      focusedPath,
      handleToggle,
      onDocumentSelect,
      visibleTreeNodes,
    ],
  );

  if (loading) {
    return (
      <div data-testid="document-tree-loading">
        <SkeletonStack
          className={styles.loadingList}
          lineClassNames={DOCUMENT_TREE_LOADING_LINE_CLASSNAMES}
        />
      </div>
    );
  }

  if (documents.length === 0) {
    return (
      <div data-testid="document-tree-empty">
        <p className={styles.emptyMessage}>No documents yet.</p>
      </div>
    );
  }

  return (
    <nav aria-label="Document tree" data-testid="document-tree">
      {/* biome-ignore lint/a11y/noNoninteractiveElementToInteractiveRole: role="tree" is correct for ARIA tree widget */}
      <ul
        className={styles.tree}
        onKeyDown={handleTreeKeyDown}
        ref={treeRef}
        role="tree"
      >
        {orderedTree.map((node) => (
          <TreeNodeItem
            activeDocumentId={activeDocumentId}
            draggingDocumentPath={draggingDocumentPath}
            dropTargetPath={dropTargetPath}
            editingDocumentId={editingDocumentId}
            editingPath={editingPath}
            expanded={expanded}
            focusedPath={focusedPath}
            key={node.fullPath}
            node={node}
            onContextAction={handleContextAction}
            onDragEndDocument={clearDragState}
            onDragEnterTarget={setDropTargetPath}
            onDragStartDocument={(dragNode) =>
              setDraggingDocumentPath(dragNode.fullPath)
            }
            onDocumentSelect={onDocumentSelect}
            onFolderSelect={onFolderSelect}
            onDropOnFile={handleDropOnFile}
            onDropOnFolder={handleDropOnFolder}
            onFocusPath={setFocusedPath}
            onRenameCancel={cancelRenameDocument}
            onRenameChange={setEditingPath}
            onRenameCommit={commitRenameDocument}
            onToggle={handleToggle}
            selectedFolderPath={selectedFolderPath}
          />
        ))}
      </ul>
    </nav>
  );
}
