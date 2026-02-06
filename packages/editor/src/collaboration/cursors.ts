// Remote cursor rendering: colored cursors with auto-hiding name labels.
// Reads awareness state from a Yjs Awareness instance and renders
// CodeMirror 6 widget decorations for each remote peer.

import {
  type EditorState,
  StateEffect,
  StateField,
  type Transaction,
} from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
} from "@codemirror/view";
import type { Awareness } from "y-protocols/awareness";

/** Milliseconds before a name label auto-hides after last cursor movement. */
const LABEL_HIDE_DELAY_MS = 3_000;

/** Number of preset cursor colors to cycle through. */
const CURSOR_PALETTE_SIZE = 12;

/**
 * A bright, high-contrast palette for remote cursors.
 * 12 hues spread evenly across the color wheel.
 */
const CURSOR_PALETTE: readonly string[] = [
  "#e06c75", // red
  "#e5c07b", // yellow
  "#98c379", // green
  "#56b6c2", // cyan
  "#61afef", // blue
  "#c678dd", // purple
  "#d19a66", // orange
  "#be5046", // rust
  "#7ec699", // mint
  "#e06ca0", // pink
  "#5fb3b3", // teal
  "#c8ae9d", // tan
];

// ── Color from name ──────────────────────────────────────────────────

/**
 * Deterministic color assignment from a name string.
 * Uses a simple hash so the same name always gets the same color,
 * consistent across sessions and devices.
 */
export function nameToColor(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = (hash * 31 + name.charCodeAt(i)) | 0;
  }
  return CURSOR_PALETTE[Math.abs(hash) % CURSOR_PALETTE_SIZE];
}

// ── Types ────────────────────────────────────────────────────────────

interface RemotePeer {
  readonly clientId: number;
  readonly name: string;
  readonly color: string;
  readonly cursor: number; // absolute position in document
  readonly lastMovedAt: number; // timestamp of last cursor change
}

// ── State effect for awareness updates ───────────────────────────────

const setRemotePeers = StateEffect.define<readonly RemotePeer[]>();

// ── Cursor widget ────────────────────────────────────────────────────

class CursorWidget extends WidgetType {
  constructor(
    readonly name: string,
    readonly color: string,
    readonly showLabel: boolean,
  ) {
    super();
  }

  eq(other: CursorWidget): boolean {
    return (
      this.name === other.name &&
      this.color === other.color &&
      this.showLabel === other.showLabel
    );
  }

  toDOM(): HTMLElement {
    const wrapper = document.createElement("span");
    wrapper.className = "cm-remote-cursor";
    wrapper.setAttribute("aria-hidden", "true");

    const line = document.createElement("span");
    line.className = "cm-remote-cursor-line";
    line.style.borderLeftColor = this.color;
    wrapper.appendChild(line);

    if (this.showLabel) {
      const label = document.createElement("span");
      label.className = "cm-remote-cursor-label";
      label.style.backgroundColor = this.color;
      label.textContent = this.name;
      wrapper.appendChild(label);
    }

    return wrapper;
  }

  ignoreEvent(): boolean {
    return true;
  }
}

// ── State field: remote peer list ────────────────────────────────────

const remotePeersField = StateField.define<readonly RemotePeer[]>({
  create() {
    return [];
  },
  update(peers: readonly RemotePeer[], tr: Transaction) {
    for (const effect of tr.effects) {
      if (effect.is(setRemotePeers)) {
        return effect.value;
      }
    }
    return peers;
  },
});

// ── Decoration builder ───────────────────────────────────────────────

function buildDecorations(
  state: EditorState,
  now: number,
): DecorationSet {
  const peers = state.field(remotePeersField);
  if (peers.length === 0) {
    return Decoration.none;
  }

  const docLength = state.doc.length;
  const decorations: { pos: number; decoration: Decoration }[] = [];

  for (const peer of peers) {
    const pos = Math.min(Math.max(0, peer.cursor), docLength);
    const showLabel = now - peer.lastMovedAt < LABEL_HIDE_DELAY_MS;

    decorations.push({
      pos,
      decoration: Decoration.widget({
        widget: new CursorWidget(peer.name, peer.color, showLabel),
        side: 1,
      }),
    });
  }

  // Decorations must be sorted by position for CodeMirror.
  decorations.sort((a, b) => a.pos - b.pos);

  return Decoration.set(
    decorations.map(({ pos, decoration }) => decoration.range(pos)),
  );
}

// ── View plugin: awareness → state → decorations ─────────────────────

/**
 * Options for the remote cursor extension.
 */
export interface RemoteCursorOptions {
  /** The Yjs Awareness instance to read peer state from. */
  readonly awareness: Awareness;
  /**
   * Override the time source (for testing). Returns milliseconds since epoch.
   * Defaults to `Date.now`.
   */
  readonly now?: () => number;
}

/**
 * Creates a CodeMirror 6 extension that renders remote cursors with
 * name labels. Labels auto-hide after 3 seconds of inactivity.
 *
 * Usage:
 * ```ts
 * const ext = remoteCursorExtension({ awareness });
 * ```
 */
export function remoteCursorExtension(
  options: RemoteCursorOptions,
): import("@codemirror/state").Extension {
  const { awareness } = options;
  const nowFn = options.now ?? Date.now;

  /** Track the last known cursor position per client for move detection. */
  const lastKnownCursor = new Map<number, number>();

  /** Track when each peer's cursor last moved. */
  const lastMovedAt = new Map<number, number>();

  function readPeers(): readonly RemotePeer[] {
    const localClientId = awareness.clientID;
    const now = nowFn();
    const peers: RemotePeer[] = [];

    for (const [clientId, state] of awareness.getStates()) {
      if (clientId === localClientId) continue;

      // y-codemirror.next stores cursor info in state.cursor
      const cursor = state.cursor;
      if (cursor == null) continue;

      // anchor is the cursor position in Yjs-relative offset; if it's
      // an absolute position we use it directly. y-codemirror.next
      // encodes the selection as {anchor, head}. For a cursor (no selection),
      // anchor === head. We use anchor as the position.
      const pos =
        typeof cursor.anchor === "number" ? cursor.anchor : undefined;
      if (pos == null) continue;

      const name: string = state.user?.name ?? `User ${clientId}`;
      const color: string = state.user?.color ?? nameToColor(name);

      // Detect cursor movement.
      const prevPos = lastKnownCursor.get(clientId);
      if (prevPos !== pos) {
        lastMovedAt.set(clientId, now);
        lastKnownCursor.set(clientId, pos);
      }

      peers.push({
        clientId,
        name,
        color,
        cursor: pos,
        lastMovedAt: lastMovedAt.get(clientId) ?? now,
      });
    }

    // Clean up entries for peers that have left.
    for (const id of lastKnownCursor.keys()) {
      if (!awareness.getStates().has(id)) {
        lastKnownCursor.delete(id);
        lastMovedAt.delete(id);
      }
    }

    return peers;
  }

  const plugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      private hideTimer: ReturnType<typeof setTimeout> | null = null;
      private awarenessHandler: () => void;

      constructor(private view: EditorView) {
        this.decorations = buildDecorations(
          this.view.state,
          nowFn(),
        );

        this.awarenessHandler = () => {
          this.syncPeers();
        };
        awareness.on("change", this.awarenessHandler);

        // Schedule the first label-hide check.
        this.scheduleHideCheck();
      }

      update(update: ViewUpdate): void {
        if (
          update.transactions.some((tr) =>
            tr.effects.some((e) => e.is(setRemotePeers)),
          )
        ) {
          this.decorations = buildDecorations(update.state, nowFn());
        }
      }

      destroy(): void {
        awareness.off("change", this.awarenessHandler);
        if (this.hideTimer != null) {
          clearTimeout(this.hideTimer);
        }
      }

      private syncPeers(): void {
        const peers = readPeers();
        this.view.dispatch({
          effects: setRemotePeers.of(peers),
        });
        this.scheduleHideCheck();
      }

      private scheduleHideCheck(): void {
        if (this.hideTimer != null) {
          clearTimeout(this.hideTimer);
        }
        // Re-render after LABEL_HIDE_DELAY_MS to hide stale labels.
        this.hideTimer = setTimeout(() => {
          this.decorations = buildDecorations(
            this.view.state,
            nowFn(),
          );
          // Force a decoration update by re-dispatching current peers.
          const peers = this.view.state.field(remotePeersField);
          this.view.dispatch({
            effects: setRemotePeers.of(peers),
          });
        }, LABEL_HIDE_DELAY_MS + 50);
      }
    },
    {
      decorations: (v) => v.decorations,
    },
  );

  return [remotePeersField, plugin, remoteCursorBaseTheme];
}

// ── Base theme ───────────────────────────────────────────────────────

const remoteCursorBaseTheme = EditorView.baseTheme({
  ".cm-remote-cursor": {
    position: "relative",
    display: "inline",
    pointerEvents: "none",
  },
  ".cm-remote-cursor-line": {
    position: "absolute",
    borderLeft: "2px solid",
    height: "1.2em",
    top: "0",
    marginLeft: "-1px",
    zIndex: "10",
  },
  ".cm-remote-cursor-label": {
    position: "absolute",
    top: "-1.4em",
    left: "-1px",
    fontSize: "0.7em",
    lineHeight: "1.2",
    padding: "1px 4px",
    borderRadius: "3px",
    color: "white",
    whiteSpace: "nowrap",
    pointerEvents: "none",
    zIndex: "11",
    fontFamily: "sans-serif",
    fontWeight: "500",
  },
});

// ── Exports for testing ──────────────────────────────────────────────

export {
  LABEL_HIDE_DELAY_MS,
  remotePeersField,
  setRemotePeers,
  type RemotePeer,
};
