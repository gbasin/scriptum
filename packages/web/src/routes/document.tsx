import { useEffect, useState } from "react";
import { useParams } from "react-router-dom";
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

  useEffect(() => {
    const api = window.__SCRIPTUM_TEST__;
    if (!api) {
      return;
    }

    setFixtureState(api.getState());
    return api.subscribe((nextState) => setFixtureState(nextState));
  }, []);

  return (
    <section aria-label="Document workspace">
      <h1 data-testid="document-title">
        Document: {workspaceId ?? "unknown"}/{documentId ?? "unknown"}
      </h1>
      <p aria-label="Sync state" data-testid="sync-state" role="status">
        Sync: {fixtureState.syncState}
      </p>

      <section aria-label="Editor surface" data-testid="editor-surface">
        <h2>Editor</h2>
        <pre data-testid="editor-content">{fixtureState.docContent}</pre>
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
    </section>
  );
}
