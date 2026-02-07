import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import {
  type EditorState,
  type Extension,
  RangeSetBuilder,
  StateField,
} from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  WidgetType,
} from "@codemirror/view";
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
  mathPreviewDecorations,
  mathPreviewTheme,
} from "./math";
import {
  taskBlockquoteHrDecorations,
  taskBlockquoteHrTheme,
} from "./task-list";
import {
  activeLines,
  getGlobalRecord,
  lineIsInActiveSelection,
  markdownTreeField,
  rangeTouchesActiveLine,
} from "./shared";

const IMAGE_PATTERN = /!\[([^\]]*)\]\(([^)]+)\)/g;
const LINK_PATTERN = /\[([^\]]+)\]\(([^)]+)\)/g;
const AUTOLINK_PATTERN = /<((?:https?|mailto):[^>\s]+)>/g;

interface InlinePreviewToken {
  readonly from: number;
  readonly href: string;
  readonly kind: "image" | "link";
  readonly label: string;
  readonly to: number;
}

function parseDestination(rawDestination: string): string {
  const trimmed = rawDestination.trim();
  if (trimmed.length === 0) {
    return "";
  }

  const unwrapped =
    trimmed.startsWith("<") && trimmed.endsWith(">")
      ? trimmed.slice(1, -1)
      : trimmed;
  const separator = unwrapped.search(/\s/);
  return (separator === -1 ? unwrapped : unwrapped.slice(0, separator)).trim();
}

function sanitizePreviewHref(rawHref: string, kind: "image" | "link"): string {
  const href = rawHref.trim();
  if (!href) {
    return "";
  }

  const lowered = href.toLowerCase();
  const schemeMatch = lowered.match(/^([a-z][a-z0-9+.-]*):/);
  if (!schemeMatch) {
    return href;
  }

  const scheme = schemeMatch[1];
  if (
    scheme === "http" ||
    scheme === "https" ||
    scheme === "mailto" ||
    scheme === "tel"
  ) {
    return href;
  }

  if (kind === "image" && scheme === "blob") {
    return href;
  }

  return "";
}

function extractInlinePreviewTokens(
  lineText: string,
  lineFrom: number,
): InlinePreviewToken[] {
  const tokens: InlinePreviewToken[] = [];

  for (const match of lineText.matchAll(IMAGE_PATTERN)) {
    const index = match.index ?? -1;
    if (index < 0) {
      continue;
    }

    const href = sanitizePreviewHref(parseDestination(match[2] ?? ""), "image");
    if (!href) {
      continue;
    }

    const fullMatch = match[0];
    tokens.push({
      from: lineFrom + index,
      href,
      kind: "image",
      label: match[1] ?? "",
      to: lineFrom + index + fullMatch.length,
    });
  }

  for (const match of lineText.matchAll(LINK_PATTERN)) {
    const index = match.index ?? -1;
    if (index < 0) {
      continue;
    }
    if (index > 0 && lineText[index - 1] === "!") {
      continue;
    }

    const href = sanitizePreviewHref(parseDestination(match[2] ?? ""), "link");
    if (!href) {
      continue;
    }

    const fullMatch = match[0];
    tokens.push({
      from: lineFrom + index,
      href,
      kind: "link",
      label: match[1] ?? href,
      to: lineFrom + index + fullMatch.length,
    });
  }

  for (const match of lineText.matchAll(AUTOLINK_PATTERN)) {
    const index = match.index ?? -1;
    if (index < 0) {
      continue;
    }

    const href = sanitizePreviewHref((match[1] ?? "").trim(), "link");
    if (!href) {
      continue;
    }

    const fullMatch = match[0];
    tokens.push({
      from: lineFrom + index,
      href,
      kind: "link",
      label: href,
      to: lineFrom + index + fullMatch.length,
    });
  }

  tokens.sort((left, right) => left.from - right.from || left.to - right.to);
  const filteredTokens: InlinePreviewToken[] = [];
  let lastEnd = -1;
  for (const token of tokens) {
    if (token.from < lastEnd) {
      continue;
    }
    filteredTokens.push(token);
    lastEnd = token.to;
  }

  return filteredTokens;
}

class InlineLinkWidget extends WidgetType {
  readonly kind = "link";

  constructor(
    readonly label: string,
    readonly href: string,
  ) {
    super();
  }

  eq(other: WidgetType): boolean {
    return (
      other instanceof InlineLinkWidget &&
      other.label === this.label &&
      other.href === this.href
    );
  }

  toDOM(): HTMLElement {
    const anchor = document.createElement("a");
    anchor.className = "cm-livePreview-link";
    anchor.href = this.href;
    anchor.rel = "noreferrer noopener";
    anchor.target = "_blank";
    anchor.textContent = this.label;
    return anchor;
  }
}

class InlineImageWidget extends WidgetType {
  readonly kind = "image";

  constructor(
    readonly alt: string,
    readonly src: string,
  ) {
    super();
  }

  eq(other: WidgetType): boolean {
    return (
      other instanceof InlineImageWidget &&
      other.alt === this.alt &&
      other.src === this.src
    );
  }

  toDOM(): HTMLElement {
    const wrapper = document.createElement("span");
    wrapper.className = "cm-livePreview-imageWrapper";

    const image = document.createElement("img");
    image.alt = this.alt;
    image.className = "cm-livePreview-image";
    image.src = this.src;
    wrapper.appendChild(image);

    if (this.alt.trim().length > 0) {
      const alt = document.createElement("span");
      alt.className = "cm-livePreview-imageAlt";
      alt.textContent = this.alt;
      wrapper.appendChild(alt);
    }

    return wrapper;
  }
}

function buildInlineLinkDecorations(state: EditorState): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();

  for (let lineNumber = 1; lineNumber <= state.doc.lines; lineNumber += 1) {
    if (lineIsInActiveSelection(state, lineNumber)) {
      continue;
    }

    const line = state.doc.line(lineNumber);
    const tokens = extractInlinePreviewTokens(line.text, line.from);
    for (const token of tokens) {
      builder.add(
        token.from,
        token.to,
        Decoration.replace({
          inclusive: false,
          widget:
            token.kind === "image"
              ? new InlineImageWidget(token.label, token.href)
              : new InlineLinkWidget(token.label, token.href),
        }),
      );
    }
  }

  return builder.finish();
}

type TableAlignment = "center" | "left" | "right";

interface ParsedTableBlock {
  readonly alignments: readonly TableAlignment[];
  readonly from: number;
  readonly headers: readonly string[];
  readonly nextLine: number;
  readonly rows: readonly (readonly string[])[];
  readonly to: number;
}

function parseTableCells(lineText: string): string[] | null {
  if (!lineText.includes("|")) {
    return null;
  }

  const trimmed = lineText.trim();
  if (trimmed.length === 0) {
    return null;
  }

  const withoutLeading = trimmed.startsWith("|") ? trimmed.slice(1) : trimmed;
  const withoutEdges = withoutLeading.endsWith("|")
    ? withoutLeading.slice(0, -1)
    : withoutLeading;
  const cells = withoutEdges.split("|").map((cell) => cell.trim());

  if (cells.length === 0 || cells.every((cell) => cell.length === 0)) {
    return null;
  }

  return cells;
}

function parseAlignmentRow(separatorText: string): TableAlignment[] | null {
  const cells = parseTableCells(separatorText);
  if (!cells || cells.length === 0) {
    return null;
  }

  const alignments: TableAlignment[] = [];
  for (const cell of cells) {
    const marker = cell.replace(/\s+/g, "");
    if (!/^:?-{3,}:?$/.test(marker)) {
      return null;
    }

    if (marker.startsWith(":") && marker.endsWith(":")) {
      alignments.push("center");
    } else if (marker.endsWith(":")) {
      alignments.push("right");
    } else {
      alignments.push("left");
    }
  }

  return alignments;
}

function normalizeTableRow(
  row: readonly string[],
  columnCount: number,
): string[] {
  const normalized = row.slice(0, columnCount);
  while (normalized.length < columnCount) {
    normalized.push("");
  }
  return normalized;
}

function parseTableBlock(
  state: EditorState,
  startLineNumber: number,
): ParsedTableBlock | null {
  if (startLineNumber + 1 > state.doc.lines) {
    return null;
  }

  const headerLine = state.doc.line(startLineNumber);
  const separatorLine = state.doc.line(startLineNumber + 1);
  const headers = parseTableCells(headerLine.text);
  const alignments = parseAlignmentRow(separatorLine.text);
  if (!headers || !alignments || headers.length === 0) {
    return null;
  }

  const columnCount = Math.min(headers.length, alignments.length);
  if (columnCount === 0) {
    return null;
  }

  const normalizedHeaders = normalizeTableRow(headers, columnCount);
  const normalizedAlignments = alignments.slice(0, columnCount);
  const rows: string[][] = [];

  let lineNumber = startLineNumber + 2;
  let blockEndLine = startLineNumber + 1;
  while (lineNumber <= state.doc.lines) {
    const rowLine = state.doc.line(lineNumber);
    const rowCells = parseTableCells(rowLine.text);
    if (!rowCells) {
      break;
    }

    rows.push(normalizeTableRow(rowCells, columnCount));
    blockEndLine = lineNumber;
    lineNumber += 1;
  }

  const blockEnd = state.doc.line(blockEndLine);
  return {
    alignments: normalizedAlignments,
    from: headerLine.from,
    headers: normalizedHeaders,
    nextLine: lineNumber,
    rows,
    to: blockEnd.to,
  };
}

class InlineTableWidget extends WidgetType {
  readonly kind = "table";

  constructor(
    readonly headers: readonly string[],
    readonly rows: readonly (readonly string[])[],
    readonly alignments: readonly TableAlignment[],
  ) {
    super();
  }

  eq(other: WidgetType): boolean {
    return (
      other instanceof InlineTableWidget &&
      JSON.stringify(other.headers) === JSON.stringify(this.headers) &&
      JSON.stringify(other.rows) === JSON.stringify(this.rows) &&
      JSON.stringify(other.alignments) === JSON.stringify(this.alignments)
    );
  }

  toDOM(): HTMLElement {
    const wrapper = document.createElement("div");
    wrapper.className = "cm-livePreview-tableWrapper";

    const table = document.createElement("table");
    table.className = "cm-livePreview-table";

    const head = document.createElement("thead");
    const headRow = document.createElement("tr");
    this.headers.forEach((header, index) => {
      const cell = document.createElement("th");
      cell.className = `cm-livePreview-tableCell cm-livePreview-tableCell-${this.alignments[index]}`;
      cell.textContent = header;
      headRow.appendChild(cell);
    });
    head.appendChild(headRow);
    table.appendChild(head);

    if (this.rows.length > 0) {
      const body = document.createElement("tbody");
      for (const row of this.rows) {
        const rowElement = document.createElement("tr");
        row.forEach((value, index) => {
          const cell = document.createElement("td");
          cell.className = `cm-livePreview-tableCell cm-livePreview-tableCell-${this.alignments[index]}`;
          cell.textContent = value;
          rowElement.appendChild(cell);
        });
        body.appendChild(rowElement);
      }
      table.appendChild(body);
    }

    wrapper.appendChild(table);
    return wrapper;
  }
}

function buildTableDecorations(state: EditorState): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();

  for (let lineNumber = 1; lineNumber <= state.doc.lines; ) {
    const tableBlock = parseTableBlock(state, lineNumber);
    if (!tableBlock) {
      lineNumber += 1;
      continue;
    }

    if (rangeTouchesActiveLine(state, tableBlock.from, tableBlock.to)) {
      lineNumber = tableBlock.nextLine;
      continue;
    }

    builder.add(
      tableBlock.from,
      tableBlock.to,
      Decoration.replace({
        block: true,
        inclusive: false,
        widget: new InlineTableWidget(
          tableBlock.headers,
          tableBlock.rows,
          tableBlock.alignments,
        ),
      }),
    );
    lineNumber = tableBlock.nextLine;
  }

  return builder.finish();
}


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

const inlineLinkTheme = EditorView.baseTheme({
  ".cm-livePreview-link": {
    color: "#2563eb",
    textDecoration: "underline",
    textUnderlineOffset: "0.12em",
  },
  ".cm-livePreview-link:hover": {
    color: "#1d4ed8",
  },
  ".cm-livePreview-imageWrapper": {
    alignItems: "flex-start",
    display: "inline-flex",
    flexDirection: "column",
    gap: "0.2rem",
    maxWidth: "100%",
    verticalAlign: "text-top",
  },
  ".cm-livePreview-image": {
    border: "1px solid #e5e7eb",
    borderRadius: "0.35rem",
    maxHeight: "12rem",
    maxWidth: "16rem",
    objectFit: "contain",
  },
  ".cm-livePreview-imageAlt": {
    color: "#6b7280",
    fontSize: "0.75em",
  },
});

const tablePreviewTheme = EditorView.baseTheme({
  ".cm-livePreview-tableWrapper": {
    margin: "0.35rem 0",
    overflowX: "auto",
  },
  ".cm-livePreview-table": {
    borderCollapse: "collapse",
    fontSize: "0.95em",
    minWidth: "18rem",
    width: "100%",
  },
  ".cm-livePreview-tableCell": {
    border: "1px solid #e5e7eb",
    padding: "0.3rem 0.5rem",
  },
  ".cm-livePreview-table th.cm-livePreview-tableCell": {
    backgroundColor: "#f8fafc",
    fontWeight: "650",
  },
  ".cm-livePreview-tableCell-left": {
    textAlign: "left",
  },
  ".cm-livePreview-tableCell-center": {
    textAlign: "center",
  },
  ".cm-livePreview-tableCell-right": {
    textAlign: "right",
  },
});


export const inlineLinkDecorations = StateField.define<DecorationSet>({
  create: buildInlineLinkDecorations,
  update(currentDecorations, transaction) {
    if (!transaction.docChanged && !transaction.selection) {
      return currentDecorations;
    }

    return buildInlineLinkDecorations(transaction.state);
  },
  provide: (field) => EditorView.decorations.from(field),
});

export const tablePreviewDecorations = StateField.define<DecorationSet>({
  create: buildTableDecorations,
  update(currentDecorations, transaction) {
    if (!transaction.docChanged && !transaction.selection) {
      return currentDecorations;
    }

    return buildTableDecorations(transaction.state);
  },
  provide: (field) => EditorView.decorations.from(field),
});
export {
  codeBlockDecorations,
} from "./code-block";
export {
  inlineEmphasisDecorations,
} from "./emphasis";
export {
  mathPreviewDecorations,
} from "./math";
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
