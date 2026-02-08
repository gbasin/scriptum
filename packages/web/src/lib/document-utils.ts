import type { Document as ScriptumDocument } from "@scriptum/shared";
import type { OpenDocumentTab } from "../components/editor/TabBar";

function documentTitleFromPath(path: string): string {
  const segments = path
    .split("/")
    .map((segment) => segment.trim())
    .filter((segment) => segment.length > 0);
  return segments[segments.length - 1] ?? path;
}

export function buildOpenDocumentTabs(
  openDocuments: readonly ScriptumDocument[],
  workspaceId: string | undefined,
  activeDocumentId: string | undefined,
  activeDocumentPath: string,
): OpenDocumentTab[] {
  const workspaceOpenDocuments = workspaceId
    ? openDocuments.filter((document) => document.workspaceId === workspaceId)
    : [];
  const tabs = workspaceOpenDocuments.map((document) => ({
    id: document.id,
    path: document.path,
    title: document.title,
  }));

  if (activeDocumentId && !tabs.some((tab) => tab.id === activeDocumentId)) {
    tabs.unshift({
      id: activeDocumentId,
      path: activeDocumentPath,
      title: documentTitleFromPath(activeDocumentPath),
    });
  }

  return tabs;
}

export function nextDocumentIdAfterClose(
  tabs: readonly OpenDocumentTab[],
  closingDocumentId: string,
): string | null {
  const closingIndex = tabs.findIndex((tab) => tab.id === closingDocumentId);
  if (closingIndex < 0) {
    return null;
  }

  const remainingTabs = tabs.filter((tab) => tab.id !== closingDocumentId);
  if (remainingTabs.length === 0) {
    return null;
  }

  const nextIndex = Math.max(0, closingIndex - 1);
  return remainingTabs[nextIndex]?.id ?? remainingTabs[0]?.id ?? null;
}
