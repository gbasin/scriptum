import type { Document } from "@scriptum/shared";
import type * as Y from "yjs";
import { create, type StoreApi, type UseBoundStore } from "zustand";
import { asNullableString, asNumber, asString } from "../lib/type-guards";

const DEFAULT_DOCUMENTS_ARRAY_NAME = "documents";
const DEFAULT_OPEN_DOCUMENT_IDS_ARRAY_NAME = "openDocumentIds";
const DEFAULT_ACTIVE_DOCUMENT_BY_WORKSPACE_MAP_NAME =
  "activeDocumentByWorkspace";

interface DocumentsSnapshot {
  documents: Document[];
  openDocumentIds: string[];
  activeDocumentIdByWorkspace: Record<string, string | null>;
}

interface ResolvedDocumentsSnapshot extends DocumentsSnapshot {
  openDocuments: Document[];
}

export interface DocumentsStoreState extends ResolvedDocumentsSnapshot {
  closeDocument: (documentId: string) => void;
  openDocument: (documentId: string) => void;
  removeDocument: (documentId: string) => void;
  reset: () => void;
  setActiveDocumentForWorkspace: (
    workspaceId: string,
    documentId: string | null,
  ) => void;
  setDocuments: (documents: Document[]) => void;
  setOpenDocumentIds: (documentIds: string[]) => void;
  upsertDocument: (document: Document) => void;
}

export type DocumentsStore = UseBoundStore<StoreApi<DocumentsStoreState>>;

export interface DocumentsYjsBindingOptions {
  activeDocumentByWorkspaceMapName?: string;
  documentsArrayName?: string;
  openDocumentIdsArrayName?: string;
  store?: DocumentsStore;
}

function asStringArray(value: unknown): string[] | null {
  if (!Array.isArray(value)) {
    return null;
  }

  const values: string[] = [];
  for (const item of value) {
    if (typeof item !== "string") {
      return null;
    }
    values.push(item);
  }

  return values;
}

function normalizeDocument(value: unknown): Document | null {
  if (!value || typeof value !== "object") {
    return null;
  }

  const document = value as Record<string, unknown>;
  const id = asString(document.id);
  const workspaceId = asString(document.workspaceId);
  const path = asString(document.path);
  const title = asString(document.title);
  const bodyMdValue = document.bodyMd;
  const tags = asStringArray(document.tags);
  const headSeq = asNumber(document.headSeq);
  const etag = asString(document.etag);
  const archivedAt = asNullableString(document.archivedAt);
  const deletedAt = asNullableString(document.deletedAt);
  const createdAt = asString(document.createdAt);
  const updatedAt = asString(document.updatedAt);

  if (
    !id ||
    !workspaceId ||
    !path ||
    !title ||
    (bodyMdValue !== undefined && typeof bodyMdValue !== "string") ||
    !tags ||
    headSeq === null ||
    !etag ||
    (archivedAt === null && document.archivedAt !== null) ||
    (deletedAt === null && document.deletedAt !== null) ||
    !createdAt ||
    !updatedAt
  ) {
    return null;
  }

  return {
    id,
    workspaceId,
    path,
    title,
    ...(typeof bodyMdValue === "string" ? { bodyMd: bodyMdValue } : {}),
    tags,
    headSeq,
    etag,
    archivedAt,
    deletedAt,
    createdAt,
    updatedAt,
  };
}

function normalizeDocuments(values: readonly unknown[]): Document[] {
  const documents: Document[] = [];
  const seenDocumentIds = new Set<string>();

  for (const value of values) {
    const document = normalizeDocument(value);
    if (!document || seenDocumentIds.has(document.id)) {
      continue;
    }

    seenDocumentIds.add(document.id);
    documents.push(document);
  }

  return documents;
}

function normalizeOpenDocumentIds(
  values: readonly unknown[],
  documentsById: Map<string, Document>,
): string[] {
  const openDocumentIds: string[] = [];
  const seenOpenDocumentIds = new Set<string>();

  for (const value of values) {
    if (typeof value !== "string") {
      continue;
    }
    if (seenOpenDocumentIds.has(value) || !documentsById.has(value)) {
      continue;
    }

    seenOpenDocumentIds.add(value);
    openDocumentIds.push(value);
  }

  return openDocumentIds;
}

function normalizeActiveDocumentIdByWorkspace(
  value: Record<string, unknown>,
  documentsById: Map<string, Document>,
): Record<string, string | null> {
  const activeDocumentIdByWorkspace: Record<string, string | null> = {};

  for (const [workspaceId, documentIdValue] of Object.entries(value)) {
    if (typeof workspaceId !== "string" || workspaceId.length === 0) {
      continue;
    }

    if (documentIdValue === null) {
      activeDocumentIdByWorkspace[workspaceId] = null;
      continue;
    }

    if (typeof documentIdValue !== "string") {
      continue;
    }

    const document = documentsById.get(documentIdValue);
    activeDocumentIdByWorkspace[workspaceId] =
      document && document.workspaceId === workspaceId ? document.id : null;
  }

  return activeDocumentIdByWorkspace;
}

function resolveDocumentsSnapshot(
  snapshot: DocumentsSnapshot,
): ResolvedDocumentsSnapshot {
  const documents = snapshot.documents.slice();
  const documentsById = new Map(
    documents.map((document) => [document.id, document]),
  );
  const openDocumentIds = normalizeOpenDocumentIds(
    snapshot.openDocumentIds,
    documentsById,
  );
  const activeDocumentIdByWorkspace = normalizeActiveDocumentIdByWorkspace(
    snapshot.activeDocumentIdByWorkspace,
    documentsById,
  );
  const openDocumentIdsByWorkspace = new Map<string, string[]>();

  for (const openDocumentId of openDocumentIds) {
    const document = documentsById.get(openDocumentId);
    if (!document) {
      continue;
    }

    const workspaceOpenDocumentIds =
      openDocumentIdsByWorkspace.get(document.workspaceId) ?? [];
    workspaceOpenDocumentIds.push(document.id);
    openDocumentIdsByWorkspace.set(
      document.workspaceId,
      workspaceOpenDocumentIds,
    );
  }

  for (const [
    workspaceId,
    workspaceOpenDocumentIds,
  ] of openDocumentIdsByWorkspace) {
    const activeDocumentId = activeDocumentIdByWorkspace[workspaceId];
    if (
      !activeDocumentId ||
      !workspaceOpenDocumentIds.includes(activeDocumentId)
    ) {
      activeDocumentIdByWorkspace[workspaceId] =
        workspaceOpenDocumentIds[0] ?? null;
    }
  }

  const openDocuments = openDocumentIds
    .map((documentId) => documentsById.get(documentId))
    .filter((document): document is Document => Boolean(document));

  return {
    documents,
    openDocumentIds,
    activeDocumentIdByWorkspace,
    openDocuments,
  };
}

export function createDocumentsStore(
  initial: Partial<DocumentsSnapshot> = {},
): DocumentsStore {
  return create<DocumentsStoreState>()((set, get) => ({
    ...resolveDocumentsSnapshot({
      documents: initial.documents ?? [],
      openDocumentIds: initial.openDocumentIds ?? [],
      activeDocumentIdByWorkspace: initial.activeDocumentIdByWorkspace ?? {},
    }),
    setDocuments: (documents) => {
      const previous = get();
      set(
        resolveDocumentsSnapshot({
          documents,
          openDocumentIds: previous.openDocumentIds,
          activeDocumentIdByWorkspace: previous.activeDocumentIdByWorkspace,
        }),
      );
    },
    upsertDocument: (document) => {
      const previous = get();
      const index = previous.documents.findIndex(
        (candidate) => candidate.id === document.id,
      );
      const documents =
        index >= 0
          ? previous.documents.map((candidate) =>
              candidate.id === document.id ? document : candidate,
            )
          : [...previous.documents, document];

      set(
        resolveDocumentsSnapshot({
          documents,
          openDocumentIds: previous.openDocumentIds,
          activeDocumentIdByWorkspace: previous.activeDocumentIdByWorkspace,
        }),
      );
    },
    removeDocument: (documentId) => {
      const previous = get();
      const documents = previous.documents.filter(
        (candidate) => candidate.id !== documentId,
      );
      set(
        resolveDocumentsSnapshot({
          documents,
          openDocumentIds: previous.openDocumentIds.filter(
            (id) => id !== documentId,
          ),
          activeDocumentIdByWorkspace: previous.activeDocumentIdByWorkspace,
        }),
      );
    },
    setOpenDocumentIds: (documentIds) => {
      const previous = get();
      set(
        resolveDocumentsSnapshot({
          documents: previous.documents,
          openDocumentIds: documentIds,
          activeDocumentIdByWorkspace: previous.activeDocumentIdByWorkspace,
        }),
      );
    },
    openDocument: (documentId) => {
      const previous = get();
      const document = previous.documents.find(
        (candidate) => candidate.id === documentId,
      );
      if (!document) {
        return;
      }

      const openDocumentIds = previous.openDocumentIds.includes(documentId)
        ? previous.openDocumentIds
        : [...previous.openDocumentIds, documentId];
      const activeDocumentIdByWorkspace = {
        ...previous.activeDocumentIdByWorkspace,
      };
      activeDocumentIdByWorkspace[document.workspaceId] ??= document.id;

      set(
        resolveDocumentsSnapshot({
          documents: previous.documents,
          openDocumentIds,
          activeDocumentIdByWorkspace,
        }),
      );
    },
    closeDocument: (documentId) => {
      const previous = get();
      const openDocumentIds = previous.openDocumentIds.filter(
        (openDocumentId) => openDocumentId !== documentId,
      );

      set(
        resolveDocumentsSnapshot({
          documents: previous.documents,
          openDocumentIds,
          activeDocumentIdByWorkspace: previous.activeDocumentIdByWorkspace,
        }),
      );
    },
    setActiveDocumentForWorkspace: (workspaceId, documentId) => {
      const previous = get();
      const activeDocumentIdByWorkspace = {
        ...previous.activeDocumentIdByWorkspace,
        [workspaceId]: documentId,
      };
      const openDocumentIds = documentId
        ? previous.openDocumentIds.includes(documentId)
          ? previous.openDocumentIds
          : [...previous.openDocumentIds, documentId]
        : previous.openDocumentIds;

      set(
        resolveDocumentsSnapshot({
          documents: previous.documents,
          openDocumentIds,
          activeDocumentIdByWorkspace,
        }),
      );
    },
    reset: () =>
      set(
        resolveDocumentsSnapshot({
          documents: [],
          openDocumentIds: [],
          activeDocumentIdByWorkspace: {},
        }),
      ),
  }));
}

export const useDocumentsStore = createDocumentsStore();

export function bindDocumentsStoreToYjs(
  doc: Y.Doc,
  options: DocumentsYjsBindingOptions = {},
): () => void {
  const store = options.store ?? useDocumentsStore;
  const documentsArray = doc.getArray<unknown>(
    options.documentsArrayName ?? DEFAULT_DOCUMENTS_ARRAY_NAME,
  );
  const openDocumentIdsArray = doc.getArray<unknown>(
    options.openDocumentIdsArrayName ?? DEFAULT_OPEN_DOCUMENT_IDS_ARRAY_NAME,
  );
  const activeDocumentByWorkspace = doc.getMap<unknown>(
    options.activeDocumentByWorkspaceMapName ??
      DEFAULT_ACTIVE_DOCUMENT_BY_WORKSPACE_MAP_NAME,
  );

  const syncFromYjs = () => {
    const documents = normalizeDocuments(documentsArray.toArray());
    const documentsById = new Map(
      documents.map((document) => [document.id, document]),
    );
    const openDocumentIds = normalizeOpenDocumentIds(
      openDocumentIdsArray.toArray(),
      documentsById,
    );
    const activeDocumentIdByWorkspace = normalizeActiveDocumentIdByWorkspace(
      Object.fromEntries(activeDocumentByWorkspace.entries()),
      documentsById,
    );

    store.setState(
      resolveDocumentsSnapshot({
        documents,
        openDocumentIds,
        activeDocumentIdByWorkspace,
      }),
    );
  };

  const handleDocumentsChange = () => syncFromYjs();
  const handleOpenDocumentIdsChange = () => syncFromYjs();
  const handleActiveDocumentChange = () => syncFromYjs();

  documentsArray.observe(handleDocumentsChange);
  openDocumentIdsArray.observe(handleOpenDocumentIdsChange);
  activeDocumentByWorkspace.observe(handleActiveDocumentChange);
  syncFromYjs();

  return () => {
    documentsArray.unobserve(handleDocumentsChange);
    openDocumentIdsArray.unobserve(handleOpenDocumentIdsChange);
    activeDocumentByWorkspace.unobserve(handleActiveDocumentChange);
  };
}
