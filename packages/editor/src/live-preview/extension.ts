import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import {
  EditorState,
  RangeSetBuilder,
  StateField,
  type Extension,
} from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  WidgetType,
} from "@codemirror/view";
import type { Tree } from "@lezer/common";

export interface MarkdownTreeAnalysis {
  readonly rootNode: string;
  readonly length: number;
  readonly topLevelNodeCount: number;
}

const HEADING_PATTERN = /^(#{1,6})(\s+)(.*)$/;
const IMAGE_PATTERN = /!\[([^\]]*)\]\(([^)]+)\)/g;
const LINK_PATTERN = /\[([^\]]+)\]\(([^)]+)\)/g;
const AUTOLINK_PATTERN = /<((?:https?|mailto):[^>\s]+)>/g;

function activeLineFromState(state: EditorState): number {
  return state.doc.lineAt(state.selection.main.head).number;
}

function countTopLevelNodes(tree: Tree): number {
  let count = 0;
  let node = tree.topNode.firstChild;

  while (node) {
    count += 1;
    node = node.nextSibling;
  }

  return count;
}

function headingLevelFromLine(text: string): number | null {
  const match = HEADING_PATTERN.exec(text);
  if (!match) {
    return null;
  }

  return match[1].length;
}

function buildHeadingDecorations(state: EditorState): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  const activeLine = state.field(activeLines, false) ?? activeLineFromState(state);

  for (let lineNumber = 1; lineNumber <= state.doc.lines; lineNumber += 1) {
    if (lineNumber === activeLine) {
      continue;
    }

    const line = state.doc.line(lineNumber);
    const headingLevel = headingLevelFromLine(line.text);
    if (!headingLevel) {
      continue;
    }

    const match = HEADING_PATTERN.exec(line.text);
    if (!match) {
      continue;
    }

    const markerLength = match[1].length + match[2].length;
    const contentStart = line.from + markerLength;

    builder.add(
      line.from,
      line.from,
      Decoration.line({
        class: `cm-livePreview-heading-line cm-livePreview-heading-line-h${headingLevel}`,
      }),
    );

    if (markerLength > 0) {
      builder.add(
        line.from,
        contentStart,
        Decoration.replace({
          inclusive: false,
        }),
      );
    }

    if (contentStart < line.to) {
      builder.add(
        contentStart,
        line.to,
        Decoration.mark({
          class: `cm-livePreview-heading cm-livePreview-heading-h${headingLevel}`,
        }),
      );
    }
  }

  return builder.finish();
}

const EMPHASIS_CLASS_BY_NODE: Readonly<Record<string, string>> = {
  Emphasis: "cm-livePreview-emphasis",
  StrongEmphasis: "cm-livePreview-strong",
  Strikethrough: "cm-livePreview-strikethrough",
};

function isInlineMarkNode(name: string): boolean {
  return name === "EmphasisMark" || name === "StrikethroughMark";
}

function rangeTouchesActiveLine(
  state: EditorState,
  activeLine: number,
  from: number,
  to: number,
): boolean {
  const startLine = state.doc.lineAt(from).number;
  const endAnchor = Math.max(from, to - 1);
  const endLine = state.doc.lineAt(endAnchor).number;

  return activeLine >= startLine && activeLine <= endLine;
}

function addInlineEmphasisDecorations(
  decorations: Array<{ from: number; to: number; decoration: Decoration }>,
  state: EditorState,
  node: { from: number; to: number; firstChild: unknown; lastChild: unknown },
  className: string,
  activeLine: number,
): void {
  if (rangeTouchesActiveLine(state, activeLine, node.from, node.to)) {
    return;
  }

  let contentFrom = node.from;
  let contentTo = node.to;

  const firstChild = node.firstChild as
    | { from: number; to: number; type: { name: string } }
    | null;
  if (
    firstChild &&
    isInlineMarkNode(firstChild.type.name) &&
    firstChild.from === node.from
  ) {
    decorations.push({
      from: firstChild.from,
      to: firstChild.to,
      decoration: Decoration.replace({
        inclusive: false,
      }),
    });
    contentFrom = firstChild.to;
  }

  const lastChild = node.lastChild as
    | { from: number; to: number; type: { name: string } }
    | null;
  if (
    lastChild &&
    isInlineMarkNode(lastChild.type.name) &&
    lastChild.to === node.to
  ) {
    decorations.push({
      from: lastChild.from,
      to: lastChild.to,
      decoration: Decoration.replace({
        inclusive: false,
      }),
    });
    contentTo = lastChild.from;
  }

  if (contentFrom < contentTo) {
    decorations.push({
      from: contentFrom,
      to: contentTo,
      decoration: Decoration.mark({
        class: className,
      }),
    });
  }
}

function buildInlineEmphasisDecorations(state: EditorState): DecorationSet {
  const decorations: Array<{ from: number; to: number; decoration: Decoration }> = [];
  const activeLine = state.field(activeLines, false) ?? activeLineFromState(state);
  const tree = markdownLanguage.parser.parse(state.doc.toString());

  function walk(node: {
    type: { name: string };
    from: number;
    to: number;
    firstChild: unknown;
    nextSibling: unknown;
    lastChild: unknown;
  }): void {
    const className = EMPHASIS_CLASS_BY_NODE[node.type.name];
    if (className) {
      addInlineEmphasisDecorations(
        decorations,
        state,
        node,
        className,
        activeLine,
      );
    }

    let child = node.firstChild as
      | {
          type: { name: string };
          from: number;
          to: number;
          firstChild: unknown;
          nextSibling: unknown;
          lastChild: unknown;
        }
      | null;
    while (child) {
      walk(child);
      child = child.nextSibling as
        | {
            type: { name: string };
            from: number;
            to: number;
            firstChild: unknown;
            nextSibling: unknown;
            lastChild: unknown;
          }
        | null;
    }
  }

  walk(tree.topNode as unknown as {
    type: { name: string };
    from: number;
    to: number;
    firstChild: unknown;
    nextSibling: unknown;
    lastChild: unknown;
  });

  decorations.sort((left, right) => {
    if (left.from !== right.from) {
      return left.from - right.from;
    }
    return left.to - right.to;
  });

  return Decoration.set(
    decorations.map(({ from, to, decoration }) => decoration.range(from, to)),
    true,
  );
}

const headingPreviewTheme = EditorView.baseTheme({
  ".cm-livePreview-heading": {
    fontWeight: "600",
    letterSpacing: "-0.01em",
  },
  ".cm-livePreview-heading-h1": {
    fontSize: "1.9em",
    fontWeight: "700",
    lineHeight: "1.15",
  },
  ".cm-livePreview-heading-h2": {
    fontSize: "1.55em",
    fontWeight: "680",
    lineHeight: "1.2",
  },
  ".cm-livePreview-heading-h3": {
    fontSize: "1.35em",
    lineHeight: "1.25",
  },
  ".cm-livePreview-heading-h4": {
    fontSize: "1.2em",
    lineHeight: "1.3",
  },
  ".cm-livePreview-heading-h5": {
    fontSize: "1.05em",
    lineHeight: "1.35",
  },
  ".cm-livePreview-heading-h6": {
    fontSize: "0.95em",
    fontWeight: "650",
    lineHeight: "1.35",
    textTransform: "uppercase",
    letterSpacing: "0.03em",
  },
});

const inlineEmphasisTheme = EditorView.baseTheme({
  ".cm-livePreview-emphasis": {
    fontStyle: "italic",
  },
  ".cm-livePreview-strong": {
    fontWeight: "700",
  },
  ".cm-livePreview-strikethrough": {
    textDecoration: "line-through",
    textDecorationThickness: "from-font",
  },
});

export function getMarkdownNodes(source: string): MarkdownTreeAnalysis {
  const tree = markdownLanguage.parser.parse(source);

  return {
    rootNode: tree.type.name,
    length: tree.length,
    topLevelNodeCount: countTopLevelNodes(tree),
  };
}

export const activeLines = StateField.define<number>({
  create: activeLineFromState,
  update(currentLine, transaction) {
    if (!transaction.docChanged && !transaction.selection) {
      return currentLine;
    }

    return activeLineFromState(transaction.state);
  },
});

export const markdownTreeField = StateField.define<MarkdownTreeAnalysis>({
  create(state) {
    return getMarkdownNodes(state.doc.toString());
  },
  update(currentAnalysis, transaction) {
    if (!transaction.docChanged) {
      return currentAnalysis;
    }

    return getMarkdownNodes(transaction.state.doc.toString());
  },
});

export const headingPreviewDecorations = StateField.define<DecorationSet>({
  create: buildHeadingDecorations,
  update(currentDecorations, transaction) {
    if (!transaction.docChanged && !transaction.selection) {
      return currentDecorations;
    }

    return buildHeadingDecorations(transaction.state);
  },
  provide: (field) => EditorView.decorations.from(field),
});

export const inlineEmphasisDecorations = StateField.define<DecorationSet>({
  create: buildInlineEmphasisDecorations,
  update(currentDecorations, transaction) {
    if (!transaction.docChanged && !transaction.selection) {
      return currentDecorations;
    }

    return buildInlineEmphasisDecorations(transaction.state);
  },
  provide: (field) => EditorView.decorations.from(field),
});

export function isLineActive(state: EditorState, lineNumber: number): boolean {
  return state.field(activeLines) === lineNumber;
}

export function livePreview(): Extension {
  return [
    markdown(),
    activeLines,
    markdownTreeField,
    headingPreviewDecorations,
    inlineEmphasisDecorations,
    headingPreviewTheme,
    inlineEmphasisTheme,
  ];
}

export const activeLineField = activeLines;
export const analyzeMarkdownTree = getMarkdownNodes;
export const livePreviewExtension = livePreview;
export const parseHeadingLevel = headingLevelFromLine;
