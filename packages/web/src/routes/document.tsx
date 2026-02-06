import { markdown } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import {
  createCollaborationProvider,
  livePreviewExtension,
  remoteCursorExtension,
} from "@scriptum/editor";
import { useEffect, useMemo, useRef, useState } from "react";
import { useParams } from "react-router-dom";
import { StatusBar } from "../components/StatusBar";
import type { ScriptumTestState } from "../test/harness";

const DEFAULT_TEST_STATE: ScriptumTestState = {
  fixtureName: "default",
  docContent: "# Fixture Document",
  cursor: { line: 0, ch: 0 },
  remotePeers: [],
  syncState: "synced",
  gitStatus: { dirty: false, ahead: 0, behind: 0 },
  commentThreads: [],
};

const DEFAULT_DAEMON_WS_BASE_URL =
  (import.meta.env.VITE_SCRIPTUM_DAEMON_WS_URL as string | undefined) ??
  "ws://127.0.0.1:39091/yjs";

function readFixtureState(): ScriptumTestState {
  if (typeof window === "undefined" || !window.__SCRIPTUM_TEST__) {
    return DEFAULT_TEST_STATE;
  }
  return window.__SCRIPTUM_TEST__.getState();
}

export function DocumentRoute() {
  const { workspaceId, documentId } = useParams();
  const [fixtureState, setFixtureState] = useState<ScriptumTestState>(() =>
    readFixtureState(),
  );
  const fixtureModeEnabled =
    typeof window !== "undefined" && Boolean(window.__SCRIPTUM_TEST__);
  const activeEditors = fixtureModeEnabled ? fixtureState.remotePeers.length + 1 : 1;
  const [syncState, setSyncState] = useState<ScriptumTestState["syncState"]>(
    fixtureModeEnabled ? fixtureState.syncState : "reconnecting",
  );
  const [cursor, setCursor] = useState(fixtureState.cursor);
  const [daemonWsBaseUrl] = useState(DEFAULT_DAEMON_WS_BASE_URL);
  const editorHostRef = useRef<HTMLDivElement | null>(null);
  const editorViewRef = useRef<EditorView | null>(null);
  const collaborationProviderRef = useRef<ReturnType<typeof createCollaborationProvider> | null>(
    null,
  );
  const roomId = useMemo(
    () => `${workspaceId ?? "unknown-workspace"}:${documentId ?? "unknown-document"}`,
    [workspaceId, documentId],
  );

  useEffect(() => {
    const api = window.__SCRIPTUM_TEST__;
    if (!api) {
      return;
    }

    setFixtureState(api.getState());
    return api.subscribe((nextState) => setFixtureState(nextState));
  }, []);

  useEffect(() => {
    if (!fixtureModeEnabled) {
      return;
    }
    setSyncState(fixtureState.syncState);
    setCursor(fixtureState.cursor);
  }, [fixtureModeEnabled, fixtureState.cursor, fixtureState.syncState]);

  useEffect(() => {
    const host = editorHostRef.current;
    if (!host) {
      return;
    }

    host.innerHTML = "";
    const provider = createCollaborationProvider({
      url: daemonWsBaseUrl,
      room: roomId,
      connectOnCreate: false,
    });
    collaborationProviderRef.current = provider;

    if (fixtureState.docContent.length > 0) {
      provider.yText.insert(0, fixtureState.docContent);
    }

    provider.provider.on("status", ({ status }) => {
      if (fixtureModeEnabled) {
        return;
      }
      setSyncState(status === "connected" ? "synced" : "reconnecting");
    });
    if (!fixtureModeEnabled) {
      provider.connect();
      setSyncState("reconnecting");
    }

    const view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: fixtureState.docContent,
        extensions: [
          markdown(),
          livePreviewExtension(),
          provider.extension(),
          remoteCursorExtension({ awareness: provider.provider.awareness }),
          EditorView.lineWrapping,
          EditorView.updateListener.of((update) => {
            if (!update.selectionSet) {
              return;
            }
            const head = update.state.selection.main.head;
            const line = update.state.doc.lineAt(head);
            setCursor({ line: line.number - 1, ch: head - line.from });
          }),
        ],
      }),
    });
    editorViewRef.current = view;

    return () => {
      editorViewRef.current = null;
      collaborationProviderRef.current = null;
      view.destroy();
      provider.destroy();
    };
  }, [daemonWsBaseUrl, fixtureModeEnabled, roomId]);

  useEffect(() => {
    if (!fixtureModeEnabled) {
      return;
    }

    const view = editorViewRef.current;
    const provider = collaborationProviderRef.current;
    if (!view || !provider) {
      return;
    }

    const currentText = view.state.doc.toString();
    if (currentText !== fixtureState.docContent) {
      view.dispatch({
        changes: {
          from: 0,
          to: view.state.doc.length,
          insert: fixtureState.docContent,
        },
      });
    }

    const yLength = provider.yText.length;
    if (yLength > 0) {
      provider.yText.delete(0, yLength);
    }
    if (fixtureState.docContent.length > 0) {
      provider.yText.insert(0, fixtureState.docContent);
    }
  }, [fixtureModeEnabled, fixtureState.docContent]);

  return (
    <section aria-label="Document workspace">
      <h1 data-testid="document-title">
        Document: {workspaceId ?? "unknown"}/{documentId ?? "unknown"}
      </h1>

      <section aria-label="Editor surface" data-testid="editor-surface">
        <h2>Editor</h2>
        {fixtureModeEnabled ? (
          <pre data-testid="editor-content">{fixtureState.docContent}</pre>
        ) : null}
        <div
          data-testid="editor-host"
          ref={editorHostRef}
          style={{
            border: "1px solid #d1d5db",
            borderRadius: "0.5rem",
            minHeight: "20rem",
            overflow: "hidden",
          }}
        />
      </section>

      <section aria-label="Presence stack" data-testid="presence-stack">
        <h2>Presence</h2>
        {fixtureState.remotePeers.length === 0 ? (
          <p>No collaborators connected.</p>
        ) : (
          <ul>
            {fixtureState.remotePeers.map((peer) => (
              <li key={`${peer.name}-${peer.cursor.line}-${peer.cursor.ch}`}>
                {peer.name} ({peer.type})
              </li>
            ))}
          </ul>
        )}
      </section>

      <StatusBar
        syncState={syncState}
        cursor={cursor}
        activeEditors={activeEditors}
      />
    </section>
  );
}
