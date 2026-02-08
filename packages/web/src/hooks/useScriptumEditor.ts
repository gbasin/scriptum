import { markdown } from "@codemirror/lang-markdown";
import {
  Compartment,
  type Extension,
  EditorState,
  Transaction,
} from "@codemirror/state";
import { EditorView, lineNumbers } from "@codemirror/view";
import {
  commentGutterExtension,
  commentHighlightExtension,
  createCollaborationProvider,
  type CommentDecorationRange,
  type DropUploadProgress,
  dragDropUploadExtension,
  livePreviewExtension,
  nameToColor,
  reconciliationInlineExtension,
  remoteCursorExtension,
  setCommentGutterRanges,
  setCommentHighlightRanges,
  slashCommandsExtension,
  type WebRtcProviderFactory,
} from "@scriptum/editor";
import type { WorkspaceEditorFontFamily } from "@scriptum/shared";
import { useEffect, useRef, type MutableRefObject } from "react";

export interface ScriptumEditorRuntimeConfig {
  fontFamily: WorkspaceEditorFontFamily;
  tabSize: number;
  lineNumbers: boolean;
}

export interface ScriptumActiveTextSelection {
  from: number;
  line: number;
  selectedText: string;
  to: number;
}

interface EditorCallbacks {
  onActiveSelectionChanged: (
    selection: ScriptumActiveTextSelection | null,
  ) => void;
  onCursorChanged: (cursor: { ch: number; line: number }) => void;
  onDocContentChanged: (
    nextContent: string,
    isRemoteTransaction: boolean,
  ) => void;
  onDropUploadProgressChanged: (progress: DropUploadProgress | null) => void;
  onEditorReady: () => void;
  onEditorRuntimeError: (error: Error | null) => void;
  onSyncStateChanged: (state: "synced" | "reconnecting" | "error") => void;
}

export interface UseScriptumEditorOptions {
  commentRanges: readonly CommentDecorationRange[];
  daemonWsBaseUrl: string;
  editorRuntimeConfig: ScriptumEditorRuntimeConfig;
  fixtureDocContent: string;
  fixtureModeEnabled: boolean;
  isApplyingTimelineSnapshotRef: MutableRefObject<boolean>;
  realtimeE2eMode: boolean;
  roomId: string;
  typographyTheme: (fontFamily: WorkspaceEditorFontFamily) => Extension;
  uploadFile: (
    file: File,
    onProgress: (percent: number) => void,
  ) => Promise<{ url: string }>;
  webrtcProviderFactory: WebRtcProviderFactory | undefined;
  webrtcSignalingUrl: string | null;
  onActiveSelectionChanged: EditorCallbacks["onActiveSelectionChanged"];
  onCursorChanged: EditorCallbacks["onCursorChanged"];
  onDocContentChanged: EditorCallbacks["onDocContentChanged"];
  onDropUploadProgressChanged: EditorCallbacks["onDropUploadProgressChanged"];
  onEditorReady: EditorCallbacks["onEditorReady"];
  onEditorRuntimeError: EditorCallbacks["onEditorRuntimeError"];
  onSyncStateChanged: EditorCallbacks["onSyncStateChanged"];
}

export interface UseScriptumEditorResult {
  collaborationProviderRef: MutableRefObject<ReturnType<
    typeof createCollaborationProvider
  > | null>;
  editorHostRef: MutableRefObject<HTMLDivElement | null>;
  editorViewRef: MutableRefObject<EditorView | null>;
}

function toError(value: unknown, fallbackMessage: string): Error {
  if (value instanceof Error) {
    return value;
  }
  if (typeof value === "string" && value.length > 0) {
    return new Error(value);
  }
  return new Error(fallbackMessage);
}

export function useScriptumEditor(
  options: UseScriptumEditorOptions,
): UseScriptumEditorResult {
  const {
    commentRanges,
    daemonWsBaseUrl,
    editorRuntimeConfig,
    fixtureDocContent,
    fixtureModeEnabled,
    isApplyingTimelineSnapshotRef,
    realtimeE2eMode,
    roomId,
    typographyTheme,
    uploadFile,
    webrtcProviderFactory,
    webrtcSignalingUrl,
  } = options;

  const callbacksRef = useRef<EditorCallbacks>({
    onActiveSelectionChanged: options.onActiveSelectionChanged,
    onCursorChanged: options.onCursorChanged,
    onDocContentChanged: options.onDocContentChanged,
    onDropUploadProgressChanged: options.onDropUploadProgressChanged,
    onEditorReady: options.onEditorReady,
    onEditorRuntimeError: options.onEditorRuntimeError,
    onSyncStateChanged: options.onSyncStateChanged,
  });
  const editorHostRef = useRef<HTMLDivElement | null>(null);
  const editorViewRef = useRef<EditorView | null>(null);
  const collaborationProviderRef = useRef<ReturnType<
    typeof createCollaborationProvider
  > | null>(null);
  const editorFontCompartmentRef = useRef(new Compartment());
  const editorTabSizeCompartmentRef = useRef(new Compartment());
  const editorLineNumbersCompartmentRef = useRef(new Compartment());

  useEffect(() => {
    callbacksRef.current = {
      onActiveSelectionChanged: options.onActiveSelectionChanged,
      onCursorChanged: options.onCursorChanged,
      onDocContentChanged: options.onDocContentChanged,
      onDropUploadProgressChanged: options.onDropUploadProgressChanged,
      onEditorReady: options.onEditorReady,
      onEditorRuntimeError: options.onEditorRuntimeError,
      onSyncStateChanged: options.onSyncStateChanged,
    };
  }, [
    options.onActiveSelectionChanged,
    options.onCursorChanged,
    options.onDocContentChanged,
    options.onDropUploadProgressChanged,
    options.onEditorReady,
    options.onEditorRuntimeError,
    options.onSyncStateChanged,
  ]);

  useEffect(() => {
    const host = editorHostRef.current;
    if (!host) {
      return;
    }

    let provider: ReturnType<typeof createCollaborationProvider> | null = null;
    let view: EditorView | null = null;

    host.innerHTML = "";
    callbacksRef.current.onDropUploadProgressChanged(null);
    callbacksRef.current.onEditorRuntimeError(null);

    try {
      provider = createCollaborationProvider({
        connectOnCreate: false,
        room: roomId,
        url: daemonWsBaseUrl,
        webrtcSignalingUrl: webrtcSignalingUrl ?? undefined,
        webrtcProviderFactory,
      });
      collaborationProviderRef.current = provider;

      if (fixtureDocContent.length > 0) {
        provider.yText.insert(0, fixtureDocContent);
      }

      provider.provider.on("status", ({ status }) => {
        if (fixtureModeEnabled) {
          return;
        }
        callbacksRef.current.onSyncStateChanged(
          status === "connected" ? "synced" : "reconnecting",
        );
      });
      if (!fixtureModeEnabled) {
        provider.connect();
        callbacksRef.current.onSyncStateChanged("reconnecting");
      }
      if (realtimeE2eMode) {
        const localAwarenessName = `User ${provider.provider.awareness.clientID}`;
        provider.provider.awareness.setLocalStateField("user", {
          color: nameToColor(localAwarenessName),
          name: localAwarenessName,
          type: "human",
        });
        provider.provider.awareness.setLocalStateField("cursor", {
          anchor: 0,
          head: 0,
        });
      }

      view = new EditorView({
        parent: host,
        state: EditorState.create({
          doc: fixtureDocContent,
          extensions: [
            markdown(),
            livePreviewExtension(),
            slashCommandsExtension(),
            reconciliationInlineExtension(),
            commentHighlightExtension(),
            commentGutterExtension(),
            provider.extension(),
            remoteCursorExtension({ awareness: provider.provider.awareness }),
            dragDropUploadExtension({
              onError: (_error, _file) => {
                // Progress UI includes failure counts; no extra UI surface needed here.
              },
              onProgress: (progress) => {
                callbacksRef.current.onDropUploadProgressChanged(progress);
              },
              uploadFile,
            }),
            editorFontCompartmentRef.current.of(
              typographyTheme(editorRuntimeConfig.fontFamily),
            ),
            editorTabSizeCompartmentRef.current.of(
              EditorState.tabSize.of(editorRuntimeConfig.tabSize),
            ),
            editorLineNumbersCompartmentRef.current.of(
              editorRuntimeConfig.lineNumbers ? lineNumbers() : [],
            ),
            EditorView.lineWrapping,
            EditorView.updateListener.of((update) => {
              if (update.docChanged && !isApplyingTimelineSnapshotRef.current) {
                const nextContent = update.state.doc.toString();
                const isRemoteTransaction = update.transactions.some(
                  (transaction) =>
                    Boolean(transaction.annotation(Transaction.remote)),
                );
                callbacksRef.current.onDocContentChanged(
                  nextContent,
                  isRemoteTransaction,
                );
              }

              if (!update.selectionSet) {
                return;
              }

              const mainSelection = update.state.selection.main;
              const line = update.state.doc.lineAt(mainSelection.head);
              callbacksRef.current.onCursorChanged({
                ch: mainSelection.head - line.from,
                line: line.number - 1,
              });
              if (realtimeE2eMode) {
                provider?.provider.awareness.setLocalStateField("cursor", {
                  anchor: mainSelection.anchor,
                  head: mainSelection.head,
                });
              }

              if (mainSelection.empty) {
                callbacksRef.current.onActiveSelectionChanged(null);
                return;
              }

              const selectedText = update.state.sliceDoc(
                mainSelection.from,
                mainSelection.to,
              );
              if (selectedText.trim().length === 0) {
                callbacksRef.current.onActiveSelectionChanged(null);
                return;
              }

              callbacksRef.current.onActiveSelectionChanged({
                from: mainSelection.from,
                line: update.state.doc.lineAt(mainSelection.from).number,
                selectedText,
                to: mainSelection.to,
              });
            }),
          ],
        }),
      });
      editorViewRef.current = view;
      callbacksRef.current.onEditorReady();

      return () => {
        editorViewRef.current = null;
        collaborationProviderRef.current = null;
        view?.destroy();
        provider?.destroy();
      };
    } catch (error) {
      editorViewRef.current = null;
      collaborationProviderRef.current = null;
      view?.destroy();
      provider?.destroy();
      callbacksRef.current.onSyncStateChanged("error");
      callbacksRef.current.onEditorRuntimeError(
        toError(error, "Editor initialization failed unexpectedly."),
      );
    }
  }, [
    daemonWsBaseUrl,
    fixtureModeEnabled,
    isApplyingTimelineSnapshotRef,
    realtimeE2eMode,
    roomId,
    typographyTheme,
    uploadFile,
    webrtcProviderFactory,
    webrtcSignalingUrl,
  ]);

  useEffect(() => {
    const view = editorViewRef.current;
    if (!view) {
      return;
    }

    view.dispatch({
      effects: [
        setCommentHighlightRanges.of(commentRanges),
        setCommentGutterRanges.of(commentRanges),
      ],
    });
  }, [commentRanges]);

  useEffect(() => {
    const view = editorViewRef.current;
    if (!view) {
      return;
    }

    view.dispatch({
      effects: [
        editorFontCompartmentRef.current.reconfigure(
          typographyTheme(editorRuntimeConfig.fontFamily),
        ),
        editorTabSizeCompartmentRef.current.reconfigure(
          EditorState.tabSize.of(editorRuntimeConfig.tabSize),
        ),
        editorLineNumbersCompartmentRef.current.reconfigure(
          editorRuntimeConfig.lineNumbers ? lineNumbers() : [],
        ),
      ],
    });
  }, [
    editorRuntimeConfig.fontFamily,
    editorRuntimeConfig.lineNumbers,
    editorRuntimeConfig.tabSize,
    typographyTheme,
  ]);

  return {
    collaborationProviderRef,
    editorHostRef,
    editorViewRef,
  };
}
