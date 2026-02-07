import { markdownLanguage } from "@codemirror/lang-markdown";
import { type EditorState, StateField } from "@codemirror/state";
import type { Tree } from "@lezer/common";

export interface MarkdownTreeAnalysis {
  readonly rootNode: string;
  readonly length: number;
  readonly topLevelNodeCount: number;
}

const FENCE_DELIMITER_PATTERN = /^(?:`{3,}|~{3,})/;

export function getGlobalRecord(): Record<string, unknown> {
  return globalThis as unknown as Record<string, unknown>;
}

export function isEscapedAt(value: string, index: number): boolean {
  let backslashCount = 0;
  let current = index - 1;

  while (current >= 0 && value[current] === "\\") {
    backslashCount += 1;
    current -= 1;
  }

  return backslashCount % 2 === 1;
}

export function isFenceDelimiter(lineText: string): boolean {
  return FENCE_DELIMITER_PATTERN.test(lineText.trimStart());
}

export function activeLineFromState(state: EditorState): number {
  return state.doc.lineAt(state.selection.main.head).number;
}

export function activeSelectionLineRange(state: EditorState): {
  readonly startLine: number;
  readonly endLine: number;
} {
  const from = state.selection.main.from;
  const to = state.selection.main.to;
  const startLine = state.doc.lineAt(Math.min(from, to)).number;
  const endAnchor = Math.max(Math.min(from, to), Math.max(from, to) - 1);
  const endLine = state.doc.lineAt(endAnchor).number;
  return { startLine, endLine };
}

export function lineIsInActiveSelection(
  state: EditorState,
  lineNumber: number,
): boolean {
  const activeRange = activeSelectionLineRange(state);
  return (
    lineNumber >= activeRange.startLine && lineNumber <= activeRange.endLine
  );
}

export function countTopLevelNodes(tree: Tree): number {
  let count = 0;
  let node = tree.topNode.firstChild;

  while (node) {
    count += 1;
    node = node.nextSibling;
  }

  return count;
}

export function rangeTouchesActiveLine(
  state: EditorState,
  from: number,
  to: number,
): boolean {
  const activeRange = activeSelectionLineRange(state);
  const startLine = state.doc.lineAt(from).number;
  const endAnchor = Math.max(from, to - 1);
  const endLine = state.doc.lineAt(endAnchor).number;

  return !(endLine < activeRange.startLine || startLine > activeRange.endLine);
}

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

export function isLineActive(state: EditorState, lineNumber: number): boolean {
  return lineIsInActiveSelection(state, lineNumber);
}

export const analyzeMarkdownTree = getMarkdownNodes;
