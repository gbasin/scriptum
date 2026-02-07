import type { Document } from "@scriptum/shared";
import { describe, expect, it } from "vitest";
import * as Y from "yjs";
import { bindDocumentsStoreToYjs, createDocumentsStore } from "./documents";

const DOC_ALPHA: Document = {
  id: "doc-alpha",
  workspaceId: "ws-alpha",
  path: "notes/alpha.md",
  title: "Alpha",
  tags: ["team"],
  headSeq: 1,
  etag: "doc-alpha-v1",
  archivedAt: null,
  deletedAt: null,
  createdAt: "2026-01-01T00:00:00.000Z",
  updatedAt: "2026-01-01T00:00:00.000Z",
};

const DOC_BETA: Document = {
  id: "doc-beta",
  workspaceId: "ws-alpha",
  path: "notes/beta.md",
  title: "Beta",
  tags: ["sync"],
  headSeq: 3,
  etag: "doc-beta-v3",
  archivedAt: null,
  deletedAt: null,
  createdAt: "2026-01-02T00:00:00.000Z",
  updatedAt: "2026-01-02T00:00:00.000Z",
};

const DOC_GAMMA: Document = {
  id: "doc-gamma",
  workspaceId: "ws-beta",
  path: "journal/gamma.md",
  title: "Gamma",
  tags: [],
  headSeq: 2,
  etag: "doc-gamma-v2",
  archivedAt: null,
  deletedAt: null,
  createdAt: "2026-01-03T00:00:00.000Z",
  updatedAt: "2026-01-03T00:00:00.000Z",
};

describe("documents store", () => {
  it("tracks open docs and active docs per workspace with local actions", () => {
    const store = createDocumentsStore();
    store.getState().setDocuments([DOC_ALPHA, DOC_BETA, DOC_GAMMA]);

    store
      .getState()
      .setOpenDocumentIds([
        DOC_ALPHA.id,
        DOC_ALPHA.id,
        "missing-doc",
        DOC_BETA.id,
      ]);
    expect(store.getState().openDocumentIds).toEqual([
      DOC_ALPHA.id,
      DOC_BETA.id,
    ]);
    expect(
      store.getState().openDocuments.map((document) => document.id),
    ).toEqual([DOC_ALPHA.id, DOC_BETA.id]);
    expect(store.getState().activeDocumentIdByWorkspace["ws-alpha"]).toBe(
      DOC_ALPHA.id,
    );

    store.getState().setActiveDocumentForWorkspace("ws-alpha", DOC_BETA.id);
    expect(store.getState().activeDocumentIdByWorkspace["ws-alpha"]).toBe(
      DOC_BETA.id,
    );

    store.getState().removeDocument(DOC_BETA.id);
    expect(store.getState().documents.map((document) => document.id)).toEqual([
      DOC_ALPHA.id,
      DOC_GAMMA.id,
    ]);
    expect(store.getState().openDocumentIds).toEqual([DOC_ALPHA.id]);
    expect(store.getState().activeDocumentIdByWorkspace["ws-alpha"]).toBe(
      DOC_ALPHA.id,
    );
  });

  it("reacts to Yjs updates", () => {
    const doc = new Y.Doc();
    const store = createDocumentsStore();
    const stopBinding = bindDocumentsStoreToYjs(doc, { store });
    const documents = doc.getArray<Document>("documents");
    const openDocumentIds = doc.getArray<string>("openDocumentIds");
    const activeDocumentByWorkspace = doc.getMap<unknown>(
      "activeDocumentByWorkspace",
    );

    doc.transact(() => {
      documents.push([DOC_ALPHA, DOC_BETA, DOC_GAMMA]);
      openDocumentIds.push([DOC_BETA.id, DOC_GAMMA.id]);
      activeDocumentByWorkspace.set("ws-alpha", DOC_BETA.id);
      activeDocumentByWorkspace.set("ws-beta", DOC_GAMMA.id);
    });

    expect(store.getState().documents.map((document) => document.id)).toEqual([
      DOC_ALPHA.id,
      DOC_BETA.id,
      DOC_GAMMA.id,
    ]);
    expect(store.getState().openDocumentIds).toEqual([
      DOC_BETA.id,
      DOC_GAMMA.id,
    ]);
    expect(store.getState().activeDocumentIdByWorkspace).toEqual({
      "ws-alpha": DOC_BETA.id,
      "ws-beta": DOC_GAMMA.id,
    });

    doc.transact(() => {
      documents.delete(1, 1);
    });

    expect(store.getState().documents.map((document) => document.id)).toEqual([
      DOC_ALPHA.id,
      DOC_GAMMA.id,
    ]);
    expect(store.getState().openDocumentIds).toEqual([DOC_GAMMA.id]);
    expect(store.getState().activeDocumentIdByWorkspace["ws-alpha"]).toBeNull();

    stopBinding();
    doc.transact(() => {
      documents.delete(0, 1);
      openDocumentIds.delete(0, openDocumentIds.length);
      activeDocumentByWorkspace.set("ws-beta", null);
    });

    expect(store.getState().documents.map((document) => document.id)).toEqual([
      DOC_ALPHA.id,
      DOC_GAMMA.id,
    ]);
    expect(store.getState().openDocumentIds).toEqual([DOC_GAMMA.id]);
    expect(store.getState().activeDocumentIdByWorkspace["ws-beta"]).toBe(
      DOC_GAMMA.id,
    );
  });
});
