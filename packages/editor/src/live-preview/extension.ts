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

class TaskCheckboxWidget extends WidgetType {
  readonly kind = "task-checkbox";

  constructor(readonly checked: boolean) {
    super();
  }

  override eq(other: TaskCheckboxWidget): boolean {
    return this.checked === other.checked;
  }

  override toDOM(): HTMLElement {
    const node = document.createElement("span");
    node.className = "cm-livePreview-task-checkbox";
    node.setAttribute("aria-hidden", "true");
    node.textContent = this.checked ? "☑" : "☐";
    return node;
  }

  override ignoreEvent(): boolean {
    return true;
  }
}

class HorizontalRuleWidget extends WidgetType {
  readonly kind = "horizontal-rule";

  override eq(): boolean {
    return true;
  }

  override toDOM(): HTMLElement {
    const node = document.createElement("span");
    node.className = "cm-livePreview-hr";
    node.setAttribute("aria-hidden", "true");
    return node;
  }

  override ignoreEvent(): boolean {
    return true;
  }
}

function buildTaskBlockquoteHrDecorations(state: EditorState): DecorationSet {
  const decorations: Array<{ from: number; to: number; decoration: Decoration }> = [];
  const activeLine = state.field(activeLines, false) ?? activeLineFromState(state);
  const tree = markdownLanguage.parser.parse(state.doc.toString());

  function walk(node: {
    type: { name: string };
    from: number;
    to: number;
    firstChild: unknown;
    nextSibling: unknown;
  }): void {
    const nodeType = node.type.name;

    if (nodeType === "Blockquote" && !rangeTouchesActiveLine(state, activeLine, node.from, node.to)) {
      const lineStart = state.doc.lineAt(node.from).from;
      decorations.push({
        from: lineStart,
        to: lineStart,
        decoration: Decoration.line({
          class: "cm-livePreview-blockquote-line",
        }),
      });
    }

    if (nodeType === "QuoteMark" && !rangeTouchesActiveLine(state, activeLine, node.from, node.to)) {
      decorations.push({
        from: node.from,
        to: node.to,
        decoration: Decoration.replace({
          inclusive: false,
        }),
      });
    }

    if (nodeType === "Task" && !rangeTouchesActiveLine(state, activeLine, node.from, node.to)) {
      const marker = node.firstChild as
        | { type: { name: string }; from: number; to: number }
        | null;
      if (marker && marker.type.name === "TaskMarker") {
        const markerText = state.doc.sliceString(marker.from, marker.to).toLowerCase();
        const checked = markerText.includes("x");

        decorations.push({
          from: marker.from,
          to: marker.to,
          decoration: Decoration.replace({
            widget: new TaskCheckboxWidget(checked),
            inclusive: false,
          }),
        });

        let contentFrom = marker.to;
        if (
          contentFrom < node.to &&
          state.doc.sliceString(contentFrom, contentFrom + 1) === " "
        ) {
          contentFrom += 1;
        }

        if (contentFrom < node.to) {
          decorations.push({
            from: contentFrom,
            to: node.to,
            decoration: Decoration.mark({
              class: "cm-livePreview-task-content",
            }),
          });
        }
      }
    }

    if (
      nodeType === "HorizontalRule" &&
      !rangeTouchesActiveLine(state, activeLine, node.from, node.to)
    ) {
      decorations.push({
        from: node.from,
        to: node.to,
        decoration: Decoration.replace({
          widget: new HorizontalRuleWidget(),
          inclusive: false,
        }),
      });
    }

    let child = node.firstChild as
      | {
          type: { name: string };
          from: number;
          to: number;
          firstChild: unknown;
          nextSibling: unknown;
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

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function detectCodeLanguage(rawInfo: string): string | null {
  const firstToken = rawInfo.trim().split(/\s+/)[0] ?? "";
  if (firstToken.length === 0) {
    return null;
  }
  return firstToken.toLowerCase();
}

function highlightCode(code: string, language: string | null): string {
  const escaped = escapeHtml(code);
  if (!language) {
    return escaped;
  }

  if (language !== "js" && language !== "javascript" && language !== "ts" && language !== "typescript") {
    return escaped;
  }

  const pattern =
    /\b(const|let|var|function|return|if|else|for|while|class|new|import|from|export|type|interface|extends)\b|("(?:\\.|[^"])*"|'(?:\\.|[^'])*'|`(?:\\.|[^`])*`)|\b(\d+(?:\.\d+)?)\b/g;

  return escaped.replace(pattern, (match, keyword: string, stringLiteral: string, numberLiteral: string) => {
    if (keyword) {
      return `<span class="cm-livePreview-codeToken-keyword">${match}</span>`;
    }
    if (stringLiteral) {
      return `<span class="cm-livePreview-codeToken-string">${match}</span>`;
    }
    if (numberLiteral) {
      return `<span class="cm-livePreview-codeToken-number">${match}</span>`;
    }
    return match;
  });
}

function sanitizeLanguageClass(language: string): string {
  return language.replaceAll(/[^a-z0-9_-]/gi, "-").toLowerCase();
}

class CodeBlockWidget extends WidgetType {
  readonly kind = "code-block";

  constructor(
    readonly code: string,
    readonly language: string | null,
  ) {
    super();
  }

  override eq(other: CodeBlockWidget): boolean {
    return this.code === other.code && this.language === other.language;
  }

  override toDOM(): HTMLElement {
    const wrapper = document.createElement("span");
    wrapper.className = "cm-livePreview-codeBlock";
    wrapper.setAttribute("aria-hidden", "true");

    if (this.language) {
      const badge = document.createElement("span");
      badge.className = "cm-livePreview-codeLanguage";
      badge.textContent = this.language;
      wrapper.appendChild(badge);
    }

    const pre = document.createElement("pre");
    pre.className = "cm-livePreview-codePre";
    const codeNode = document.createElement("code");
    codeNode.className = "cm-livePreview-code";
    if (this.language) {
      codeNode.classList.add(`cm-livePreview-code-lang-${sanitizeLanguageClass(this.language)}`);
    }
    codeNode.innerHTML = highlightCode(this.code, this.language);
    pre.appendChild(codeNode);
    wrapper.appendChild(pre);

    return wrapper;
  }

  override ignoreEvent(): boolean {
    return true;
  }
}

function buildCodeBlockDecorations(state: EditorState): DecorationSet {
  const decorations: Array<{ from: number; to: number; decoration: Decoration }> = [];
  const activeLine = state.field(activeLines, false) ?? activeLineFromState(state);
  const tree = markdownLanguage.parser.parse(state.doc.toString());

  function walk(node: {
    type: { name: string };
    from: number;
    to: number;
    firstChild: unknown;
    nextSibling: unknown;
  }): void {
    if (node.type.name === "FencedCode" && !rangeTouchesActiveLine(state, activeLine, node.from, node.to)) {
      let language: string | null = null;
      let code = "";

      let child = node.firstChild as
        | {
            type: { name: string };
            from: number;
            to: number;
            firstChild: unknown;
            nextSibling: unknown;
          }
        | null;

      while (child) {
        if (child.type.name === "CodeInfo") {
          language = detectCodeLanguage(state.doc.sliceString(child.from, child.to));
        } else if (child.type.name === "CodeText") {
          code = state.doc.sliceString(child.from, child.to);
        }
        child = child.nextSibling as
          | {
              type: { name: string };
              from: number;
              to: number;
              firstChild: unknown;
              nextSibling: unknown;
            }
          | null;
      }

      decorations.push({
        from: node.from,
        to: node.to,
        decoration: Decoration.replace({
          widget: new CodeBlockWidget(code, language),
          inclusive: false,
        }),
      });
      return;
    }

    let child = node.firstChild as
      | {
          type: { name: string };
          from: number;
          to: number;
          firstChild: unknown;
          nextSibling: unknown;
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

    const href = parseDestination(match[2] ?? "");
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

    const href = parseDestination(match[2] ?? "");
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

    const href = (match[1] ?? "").trim();
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
  const activeLine = state.field(activeLines, false) ?? activeLineFromState(state);

  for (let lineNumber = 1; lineNumber <= state.doc.lines; lineNumber += 1) {
    if (lineNumber === activeLine) {
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

  const withoutLeading = trimmed.startsWith("|")
    ? trimmed.slice(1)
    : trimmed;
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

function normalizeTableRow(row: readonly string[], columnCount: number): string[] {
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
  const activeLine = state.field(activeLines, false) ?? activeLineFromState(state);

  for (let lineNumber = 1; lineNumber <= state.doc.lines; ) {
    const tableBlock = parseTableBlock(state, lineNumber);
    if (!tableBlock) {
      lineNumber += 1;
      continue;
    }

    if (rangeTouchesActiveLine(state, activeLine, tableBlock.from, tableBlock.to)) {
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

const taskBlockquoteHrTheme = EditorView.baseTheme({
  ".cm-livePreview-blockquote-line": {
    borderLeft: "3px solid #cbd5e1",
    color: "#475569",
    paddingLeft: "0.75rem",
  },
  ".cm-livePreview-task-checkbox": {
    color: "#0f766e",
    display: "inline-block",
    marginRight: "0.35rem",
    width: "1.1em",
  },
  ".cm-livePreview-task-content": {
    textDecoration: "none",
  },
  ".cm-livePreview-hr": {
    borderTop: "1px solid #d1d5db",
    display: "block",
    height: "0",
    margin: "0.45em 0",
    width: "100%",
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

export const taskBlockquoteHrDecorations = StateField.define<DecorationSet>({
  create: buildTaskBlockquoteHrDecorations,
  update(currentDecorations, transaction) {
    if (!transaction.docChanged && !transaction.selection) {
      return currentDecorations;
    }

    return buildTaskBlockquoteHrDecorations(transaction.state);
  },
  provide: (field) => EditorView.decorations.from(field),
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
    taskBlockquoteHrDecorations,
    inlineLinkDecorations,
    tablePreviewDecorations,
    headingPreviewTheme,
    inlineEmphasisTheme,
    taskBlockquoteHrTheme,
    inlineLinkTheme,
    tablePreviewTheme,
  ];
}

export const activeLineField = activeLines;
export const analyzeMarkdownTree = getMarkdownNodes;
export const livePreviewExtension = livePreview;
export const parseHeadingLevel = headingLevelFromLine;
