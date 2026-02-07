import { markdown } from "@codemirror/lang-markdown";
import { Decoration, EditorView } from "@codemirror/view";
import { EditorState } from "@codemirror/state";
import { nameToColor } from "@scriptum/editor";
import { useEffect, useMemo, useRef } from "react";

export type DiffViewMode = "authorship" | "diff";

export interface AuthorshipRange {
  from: number;
  to: number;
}

export interface InlineDiffSegment {
  kind: "unchanged" | "added" | "removed";
  text: string;
}

export interface DiffViewProps {
  currentContent: string;
  historicalContent: string;
  authorshipMap: Map<AuthorshipRange, string>;
  viewMode: DiffViewMode;
}

interface AuthorshipSegment {
  author: string;
  color: string;
  from: number;
  to: number;
}

const baseTheme = EditorView.theme({
  "&": {
    border: "1px solid #e5e7eb",
    borderRadius: "0.375rem",
    fontSize: "0.8rem",
  },
  ".cm-content": {
    fontFamily: "ui-monospace, SFMono-Regular, SFMono, Menlo, monospace",
    minHeight: "3rem",
    padding: "0.5rem",
    whiteSpace: "pre-wrap",
  },
  ".cm-line": {
    padding: "0",
  },
  ".cm-scroller": {
    overflow: "auto",
  },
  ".cm-scriptum-authorship": {
    borderRadius: "0.2rem",
  },
  ".cm-scriptum-diff-added": {
    backgroundColor: "#dcfce7",
    color: "#166534",
  },
  ".cm-scriptum-diff-removed": {
    backgroundColor: "#fee2e2",
    color: "#991b1b",
    textDecoration: "line-through",
  },
});

function colorWithAlpha(color: string, alphaHex: string): string {
  if (/^#[0-9a-fA-F]{6}$/.test(color)) {
    return `${color}${alphaHex}`;
  }
  return "rgba(148, 163, 184, 0.22)";
}

function normalizeAuthorshipSegments(
  authorshipMap: Map<AuthorshipRange, string>,
  contentLength: number,
): AuthorshipSegment[] {
  const segments: AuthorshipSegment[] = [];

  for (const [range, author] of authorshipMap.entries()) {
    if (!author || author.trim().length === 0) {
      continue;
    }
    const from = Math.max(0, Math.min(contentLength, Math.floor(range.from)));
    const to = Math.max(0, Math.min(contentLength, Math.floor(range.to)));
    if (to <= from) {
      continue;
    }
    segments.push({
      author,
      color: nameToColor(author),
      from,
      to,
    });
  }

  return segments.sort((left, right) => left.from - right.from);
}

function buildAuthorshipDecorations(
  authorshipMap: Map<AuthorshipRange, string>,
  contentLength: number,
): {
  decorations: ReturnType<typeof Decoration.set>;
  authors: Array<{ name: string; color: string }>;
} {
  const segments = normalizeAuthorshipSegments(authorshipMap, contentLength);

  // Build a deterministic legend using first appearance order.
  const authors: Array<{ name: string; color: string }> = [];
  const seen = new Set<string>();
  for (const segment of segments) {
    if (seen.has(segment.author)) {
      continue;
    }
    seen.add(segment.author);
    authors.push({ name: segment.author, color: segment.color });
  }

  // Flatten marks into a single DecorationSet to simplify extension wiring.
  const ranges = segments.map((segment) =>
    Decoration.mark({
      class: "cm-scriptum-authorship",
      attributes: {
        "data-author": segment.author,
        style: `background-color:${colorWithAlpha(
          segment.color,
          "33",
        )};color:${segment.color};`,
      },
    }).range(segment.from, segment.to),
  );

  return {
    decorations: Decoration.set(ranges, true),
    authors,
  };
}

export function buildInlineDiffSegments(
  currentContent: string,
  historicalContent: string,
): InlineDiffSegment[] {
  if (currentContent.length === 0 && historicalContent.length === 0) {
    return [];
  }
  if (currentContent === historicalContent) {
    return [{ kind: "unchanged", text: historicalContent }];
  }

  let prefixLength = 0;
  while (
    prefixLength < currentContent.length &&
    prefixLength < historicalContent.length &&
    currentContent[prefixLength] === historicalContent[prefixLength]
  ) {
    prefixLength += 1;
  }

  let suffixLength = 0;
  while (
    suffixLength < currentContent.length - prefixLength &&
    suffixLength < historicalContent.length - prefixLength &&
    currentContent[currentContent.length - 1 - suffixLength] ===
      historicalContent[historicalContent.length - 1 - suffixLength]
  ) {
    suffixLength += 1;
  }

  const unchangedPrefix = historicalContent.slice(0, prefixLength);
  const added = historicalContent.slice(
    prefixLength,
    historicalContent.length - suffixLength,
  );
  const removed = currentContent.slice(
    prefixLength,
    currentContent.length - suffixLength,
  );
  const unchangedSuffix =
    suffixLength > 0
      ? historicalContent.slice(historicalContent.length - suffixLength)
      : "";

  const segments: InlineDiffSegment[] = [];
  if (unchangedPrefix.length > 0) {
    segments.push({ kind: "unchanged", text: unchangedPrefix });
  }
  if (removed.length > 0) {
    segments.push({ kind: "removed", text: removed });
  }
  if (added.length > 0) {
    segments.push({ kind: "added", text: added });
  }
  if (unchangedSuffix.length > 0) {
    segments.push({ kind: "unchanged", text: unchangedSuffix });
  }
  return segments;
}

function buildDiffDecorations(
  segments: InlineDiffSegment[],
): ReturnType<typeof Decoration.set> {
  let cursor = 0;
  const actualRanges = [];

  for (const segment of segments) {
    const start = cursor;
    cursor += segment.text.length;
    if (segment.text.length === 0 || segment.kind === "unchanged") {
      continue;
    }

    actualRanges.push(
      Decoration.mark({
        class:
          segment.kind === "added"
            ? "cm-scriptum-diff-added"
            : "cm-scriptum-diff-removed",
        attributes: { "data-kind": segment.kind },
      }).range(start, cursor),
    );
  }

  return Decoration.set(actualRanges, true);
}

export function DiffView({
  currentContent,
  historicalContent,
  authorshipMap,
  viewMode,
}: DiffViewProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);

  const authorshipModel = useMemo(
    () => buildAuthorshipDecorations(authorshipMap, historicalContent.length),
    [authorshipMap, historicalContent.length],
  );

  const diffSegments = useMemo(
    () => buildInlineDiffSegments(currentContent, historicalContent),
    [currentContent, historicalContent],
  );
  const hasDiff = diffSegments.some((segment) => segment.kind !== "unchanged");

  const doc =
    viewMode === "authorship"
      ? historicalContent
      : diffSegments.map((segment) => segment.text).join("");
  const decorations =
    viewMode === "authorship"
      ? authorshipModel.decorations
      : buildDiffDecorations(diffSegments);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) {
      return;
    }

    viewRef.current?.destroy();
    viewRef.current = new EditorView({
      parent: host,
      state: EditorState.create({
        doc,
        extensions: [
          markdown(),
          EditorView.lineWrapping,
          EditorState.readOnly.of(true),
          EditorView.editable.of(false),
          baseTheme,
          EditorView.decorations.of(decorations),
        ],
      }),
    });

    return () => {
      viewRef.current?.destroy();
      viewRef.current = null;
    };
  }, [decorations, doc]);

  return (
    <section aria-label="History diff view" data-testid="history-diffview">
      {viewMode === "authorship" ? (
        <>
          <h3 style={{ fontSize: "0.875rem", margin: "0 0 0.375rem" }}>
            Author-colored highlights
          </h3>
          <div
            data-testid="history-diffview-authorship-legend"
            style={{
              alignItems: "center",
              display: "flex",
              flexWrap: "wrap",
              gap: "0.375rem",
              marginBottom: "0.375rem",
            }}
          >
            {authorshipModel.authors.map((author) => (
              <span
                data-testid={`history-diffview-author-${author.name}`}
                key={author.name}
                style={{
                  alignItems: "center",
                  border: `1px solid ${colorWithAlpha(author.color, "66")}`,
                  borderRadius: "9999px",
                  color: author.color,
                  display: "inline-flex",
                  fontSize: "0.7rem",
                  fontWeight: 700,
                  gap: "0.25rem",
                  padding: "0.1rem 0.4rem",
                }}
              >
                <span
                  aria-hidden="true"
                  style={{
                    background: author.color,
                    borderRadius: "9999px",
                    display: "inline-block",
                    height: "0.4rem",
                    width: "0.4rem",
                  }}
                />
                {author.name}
              </span>
            ))}
          </div>
          {historicalContent.length === 0 ? (
            <p data-testid="history-diffview-authorship-empty" style={{ margin: 0 }}>
              No content yet.
            </p>
          ) : null}
        </>
      ) : (
        <>
          <h3 style={{ fontSize: "0.875rem", margin: "0 0 0.375rem" }}>
            Diff from current
          </h3>
          {!hasDiff ? (
            <p data-testid="history-diffview-diff-empty" style={{ margin: 0 }}>
              Selected snapshot matches current version.
            </p>
          ) : null}
        </>
      )}

      <div data-testid="history-diffview-editor" ref={hostRef} />
    </section>
  );
}
