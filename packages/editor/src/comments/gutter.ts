import {
  RangeSet,
  RangeSetBuilder,
  StateEffect,
  StateField,
  type EditorState,
  type Extension,
} from "@codemirror/state";
import { EditorView, GutterMarker, gutter } from "@codemirror/view";
import {
  type CommentDecorationRange,
  type CommentDecorationStatus,
} from "./highlight";

export type { CommentDecorationRange, CommentDecorationStatus } from "./highlight";

interface CommentGutterState {
  readonly markers: RangeSet<GutterMarker>;
  readonly ranges: readonly CommentDecorationRange[];
}

const OPEN_STATUS: CommentDecorationStatus = "open";

export const setCommentGutterRanges = StateEffect.define<
  readonly CommentDecorationRange[]
>();

class CommentGutterMarker extends GutterMarker {
  constructor(readonly status: CommentDecorationStatus) {
    super();
  }

  eq(other: GutterMarker): boolean {
    return (
      other instanceof CommentGutterMarker && other.status === this.status
    );
  }

  toDOM() {
    const marker = document.createElement("span");
    marker.className = [
      "cm-commentGutterMarker",
      this.status === "resolved"
        ? "cm-commentGutterMarker-resolved"
        : "cm-commentGutterMarker-open",
    ].join(" ");
    marker.setAttribute("aria-hidden", "true");
    return marker;
  }
}

const OPEN_GUTTER_MARKER = new CommentGutterMarker("open");
const RESOLVED_GUTTER_MARKER = new CommentGutterMarker("resolved");

function sanitizeStatus(status: CommentDecorationStatus | undefined): CommentDecorationStatus {
  if (status === "resolved") {
    return status;
  }
  return OPEN_STATUS;
}

function normalizeRanges(
  state: EditorState,
  ranges: readonly CommentDecorationRange[]
): readonly CommentDecorationRange[] {
  const maxPosition = state.doc.length;
  const normalized: CommentDecorationRange[] = [];

  for (const range of ranges) {
    const from = Math.max(0, Math.min(range.from, maxPosition));
    const to = Math.max(0, Math.min(range.to, maxPosition));
    if (to <= from) {
      continue;
    }

    normalized.push({
      from,
      status: sanitizeStatus(range.status),
      threadId: range.threadId,
      to,
    });
  }

  return normalized;
}

function mapRangesThroughChanges(
  state: EditorState,
  ranges: readonly CommentDecorationRange[],
  changes: { mapPos: (pos: number, assoc?: number) => number }
): readonly CommentDecorationRange[] {
  const mappedRanges = ranges.map((range) => {
    const from = changes.mapPos(range.from, 1);
    const to = changes.mapPos(range.to, -1);
    return {
      ...range,
      from: Math.min(from, to),
      to: Math.max(from, to),
    };
  });

  return normalizeRanges(state, mappedRanges);
}

function mergeStatus(
  existingStatus: CommentDecorationStatus | undefined,
  nextStatus: CommentDecorationStatus
): CommentDecorationStatus {
  if (existingStatus === "open" || nextStatus === "open") {
    return "open";
  }
  return "resolved";
}

function buildMarkers(
  state: EditorState,
  ranges: readonly CommentDecorationRange[]
): RangeSet<GutterMarker> {
  if (ranges.length === 0) {
    return RangeSet.empty;
  }

  const statusByLine = new Map<number, CommentDecorationStatus>();

  for (const range of ranges) {
    const lineStart = state.doc.lineAt(range.from).number;
    const lineEnd = state.doc.lineAt(Math.max(range.from, range.to - 1)).number;
    for (let lineNumber = lineStart; lineNumber <= lineEnd; lineNumber += 1) {
      const current = statusByLine.get(lineNumber);
      statusByLine.set(lineNumber, mergeStatus(current, range.status));
    }
  }

  if (statusByLine.size === 0) {
    return RangeSet.empty;
  }

  const builder = new RangeSetBuilder<GutterMarker>();
  const lineNumbers = Array.from(statusByLine.keys()).sort((left, right) => left - right);

  for (const lineNumber of lineNumbers) {
    const line = state.doc.line(lineNumber);
    const status = statusByLine.get(lineNumber);
    builder.add(
      line.from,
      line.from,
      status === "resolved" ? RESOLVED_GUTTER_MARKER : OPEN_GUTTER_MARKER
    );
  }

  return builder.finish();
}

export const commentGutterState = StateField.define<CommentGutterState>({
  create: () => ({
    markers: RangeSet.empty,
    ranges: [],
  }),
  update(current, transaction) {
    let nextRanges = current.ranges;

    for (const effect of transaction.effects) {
      if (effect.is(setCommentGutterRanges)) {
        nextRanges = normalizeRanges(transaction.state, effect.value);
      }
    }

    if (nextRanges === current.ranges && transaction.docChanged) {
      nextRanges = mapRangesThroughChanges(
        transaction.state,
        current.ranges,
        transaction.changes
      );
    }

    if (nextRanges === current.ranges) {
      return current;
    }

    return {
      markers: buildMarkers(transaction.state, nextRanges),
      ranges: nextRanges,
    };
  },
});

const commentGutterTheme = EditorView.baseTheme({
  ".cm-commentGutter": {
    minWidth: "0.9rem",
  },
  ".cm-commentGutter .cm-gutterElement": {
    padding: "0 2px",
  },
  ".cm-commentGutterMarker": {
    borderRadius: "9999px",
    boxSizing: "border-box",
    display: "inline-block",
    height: "0.5rem",
    width: "0.5rem",
  },
  ".cm-commentGutterMarker-open": {
    backgroundColor: "rgba(250, 204, 21, 0.9)",
    boxShadow: "inset 0 0 0 1px rgba(161, 98, 7, 0.9)",
  },
  ".cm-commentGutterMarker-resolved": {
    backgroundColor: "rgba(156, 163, 175, 0.75)",
    boxShadow: "inset 0 0 0 1px rgba(107, 114, 128, 0.9)",
  },
});

export function commentGutterExtension(): Extension {
  return [
    commentGutterState,
    gutter({
      class: "cm-commentGutter",
      lineMarkerChange: (update) =>
        update.startState.field(commentGutterState) !==
        update.state.field(commentGutterState),
      markers: (view) => view.state.field(commentGutterState).markers,
    }),
    commentGutterTheme,
  ];
}
