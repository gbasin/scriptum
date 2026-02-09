// @vitest-environment jsdom
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { describe, expect, it } from "vitest";
import { Awareness } from "y-protocols/awareness";
import * as Y from "yjs";

import { createCollaborationProvider } from "./provider";

class FakeProvider {
  readonly awareness: Awareness;
  private statusHandler:
    | ((event: { status: "connected" | "disconnected" }) => void)
    | null = null;

  constructor(doc: Y.Doc) {
    this.awareness = new Awareness(doc);
  }

  connect(): void {
    this.statusHandler?.({ status: "connected" });
  }

  disconnect(): void {
    this.statusHandler?.({ status: "disconnected" });
  }

  destroy(): void {}

  on(
    _event: "status",
    handler: (event: { status: "connected" | "disconnected" }) => void,
  ): void {
    this.statusHandler = handler;
  }
}

function createEditorWithProvider() {
  const doc = new Y.Doc();
  const provider = createCollaborationProvider({
    url: "ws://127.0.0.1:39091/yjs",
    room: "workspace:document",
    doc,
    connectOnCreate: false,
    providerFactory: ({ doc: providerDoc }) => new FakeProvider(providerDoc),
  });

  const parent = document.createElement("div");
  document.body.appendChild(parent);

  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc: "",
      extensions: [provider.extension()],
    }),
  });

  return {
    provider,
    view,
    cleanup: () => {
      view.destroy();
      provider.destroy();
      parent.remove();
    },
  };
}

describe("createCollaborationProvider integration", () => {
  it("converts CodeMirror transactions into Yjs text updates", () => {
    const { provider, view, cleanup } = createEditorWithProvider();

    view.dispatch({
      changes: { from: 0, insert: "hello from cm" },
    });

    expect(provider.yText.toString()).toBe("hello from cm");

    cleanup();
  });

  it("applies remote Yjs updates into the CodeMirror document", async () => {
    const { provider, view, cleanup } = createEditorWithProvider();

    const remoteDoc = new Y.Doc();
    remoteDoc.getText("content").insert(0, "remote update");
    const encodedRemoteUpdate = Y.encodeStateAsUpdate(remoteDoc);
    Y.applyUpdate(provider.doc, encodedRemoteUpdate);

    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(view.state.doc.toString()).toBe("remote update");

    cleanup();
  });
});
