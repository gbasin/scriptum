import { markdown } from "@codemirror/lang-markdown";
import { type Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { footnotePreview } from "../extensions/footnotes";
import {
  codeBlockDecorations,
  codeBlockTheme,
} from "./code-block";
import {
  inlineEmphasisDecorations,
  inlineEmphasisTheme,
} from "./emphasis";
import {
  headingLevelFromLine,
  headingPreviewDecorations,
  headingPreviewTheme,
} from "./heading";
import {
  inlineLinkDecorations,
  inlineLinkTheme,
} from "./link";
import {
  mathPreviewDecorations,
  mathPreviewTheme,
} from "./math";
import {
  taskBlockquoteHrDecorations,
  taskBlockquoteHrTheme,
} from "./task-list";
import {
  tablePreviewDecorations,
  tablePreviewTheme,
} from "./table";
import {
  activeLines,
  markdownTreeField,
} from "./shared";


const mermaidPreviewTheme = EditorView.baseTheme({
  ".cm-livePreview-mermaidBlock": {
    backgroundColor: "#f8fafc",
    border: "1px solid #dbeafe",
    borderRadius: "0.45rem",
    margin: "0.45rem 0",
    overflowX: "auto",
    padding: "0.55rem 0.7rem",
  },
  ".cm-livePreview-mermaidFallbackCode": {
    color: "#0f172a",
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
    margin: "0",
    whiteSpace: "pre",
  },
});

export {
  codeBlockDecorations,
} from "./code-block";
export {
  inlineEmphasisDecorations,
} from "./emphasis";
export {
  inlineLinkDecorations,
} from "./link";
export {
  mathPreviewDecorations,
} from "./math";
export {
  tablePreviewDecorations,
} from "./table";
export {
  headingPreviewDecorations,
} from "./heading";
export {
  taskBlockquoteHrDecorations,
} from "./task-list";
export {
  activeLines,
  analyzeMarkdownTree,
  getMarkdownNodes,
  isLineActive,
  markdownTreeField,
  type MarkdownTreeAnalysis,
} from "./shared";

export function livePreview(): Extension {
  return [
    markdown(),
    footnotePreview(),
    activeLines,
    markdownTreeField,
    headingPreviewDecorations,
    inlineEmphasisDecorations,
    taskBlockquoteHrDecorations,
    codeBlockDecorations,
    mathPreviewDecorations,
    inlineLinkDecorations,
    tablePreviewDecorations,
    headingPreviewTheme,
    inlineEmphasisTheme,
    taskBlockquoteHrTheme,
    codeBlockTheme,
    mathPreviewTheme,
    mermaidPreviewTheme,
    inlineLinkTheme,
    tablePreviewTheme,
  ];
}

export const activeLineField = activeLines;
export const livePreviewExtension = livePreview;
export const parseHeadingLevel = headingLevelFromLine;
