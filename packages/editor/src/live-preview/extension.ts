import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState, StateField, type Extension } from "@codemirror/state";
import type { Tree } from "@lezer/common";

export interface MarkdownTreeAnalysis {
  readonly rootNode: string;
  readonly length: number;
  readonly topLevelNodeCount: number;
}

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
  return state.field(activeLines) === lineNumber;
}

export function livePreview(): Extension {
  return [markdown(), activeLines, markdownTreeField];
}

export const activeLineField = activeLines;
export const analyzeMarkdownTree = getMarkdownNodes;
export const livePreviewExtension = livePreview;
