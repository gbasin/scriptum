import type { RightPanelTab } from "../../store/ui";

export const COMPACT_LAYOUT_BREAKPOINT_PX = 1024;

export const RIGHT_PANEL_TAB_IDS: Record<RightPanelTab, string> = {
  backlinks: "right-panel-tab-backlinks",
  comments: "right-panel-tab-comments",
  outline: "right-panel-tab-outline",
};

export const RIGHT_PANEL_TAB_PANEL_IDS: Record<RightPanelTab, string> = {
  backlinks: "right-panel-tabpanel-backlinks",
  comments: "right-panel-tabpanel-comments",
  outline: "right-panel-tabpanel-outline",
};

export function isNewDocumentShortcut(event: KeyboardEvent): boolean {
  return (
    (event.metaKey || event.ctrlKey) &&
    !event.altKey &&
    !event.shiftKey &&
    event.key.toLowerCase() === "n"
  );
}

export function formatRenameBacklinkToast(
  updatedLinks: number,
  updatedDocuments: number,
): string {
  return `Updated ${updatedLinks} links across ${updatedDocuments} documents.`;
}

export function normalizeTag(tag: string): string {
  return tag.trim().replace(/^#+/, "");
}

export function parentFolderPath(path: string): string {
  const lastSlashIndex = path.lastIndexOf("/");
  if (lastSlashIndex < 0) {
    return "";
  }
  return path.slice(0, lastSlashIndex);
}

export function baseName(path: string): string {
  const segments = path.split("/").filter(Boolean);
  return segments[segments.length - 1] ?? path;
}
