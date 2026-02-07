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
import type { Tree } from "@lezer/common";
import { footnotePreview } from "../extensions/footnotes";

export interface MarkdownTreeAnalysis {
  readonly rootNode: string;
  readonly length: number;
  readonly topLevelNodeCount: number;
}

const HEADING_PATTERN = /^(#{1,6})(\s+)(.*)$/;
const IMAGE_PATTERN = /!\[([^\]]*)\]\(([^)]+)\)/g;
const LINK_PATTERN = /\[([^\]]+)\]\(([^)]+)\)/g;
const AUTOLINK_PATTERN = /<((?:https?|mailto):[^>\s]+)>/g;
const FENCE_DELIMITER_PATTERN = /^(?:`{3,}|~{3,})/;

interface KatexRenderOptions {
  readonly displayMode?: boolean;
  readonly throwOnError?: boolean;
}

interface KatexRenderer {
  readonly render: (
    expression: string,
    element: HTMLElement,
    options?: KatexRenderOptions,
  ) => void;
}

interface MermaidRenderObject {
  readonly bindFunctions?: (element: Element) => void;
  readonly svg: string;
}

type MermaidRenderResult =
  | MermaidRenderObject
  | Promise<MermaidRenderObject>
  | string
  | Promise<string>;

interface MermaidRenderer {
  readonly render: (id: string, source: string) => MermaidRenderResult;
}

let mermaidRenderSequence = 0;

function getGlobalRecord(): Record<string, unknown> {
  return globalThis as unknown as Record<string, unknown>;
}

function getKatexRenderer(): KatexRenderer | null {
  const globalRecord = getGlobalRecord();
  const candidate = globalRecord.katex;
  if (!candidate || typeof candidate !== "object") {
    return null;
  }

  const renderer = candidate as { render?: unknown };
  if (typeof renderer.render !== "function") {
    return null;
  }

  return renderer as unknown as KatexRenderer;
}

function getMermaidRenderer(): MermaidRenderer | null {
  const globalRecord = getGlobalRecord();
  const candidate = globalRecord.mermaid;
  if (!candidate || typeof candidate !== "object") {
    return null;
  }

  const renderer = candidate as { render?: unknown };
  if (typeof renderer.render !== "function") {
    return null;
  }

  return renderer as unknown as MermaidRenderer;
}

function isEscapedAt(value: string, index: number): boolean {
  let backslashCount = 0;
  let current = index - 1;

  while (current >= 0 && value[current] === "\\") {
    backslashCount += 1;
    current -= 1;
  }

  return backslashCount % 2 === 1;
}

function isFenceDelimiter(lineText: string): boolean {
  return FENCE_DELIMITER_PATTERN.test(lineText.trimStart());
}

function nextMermaidRenderId(): string {
  mermaidRenderSequence += 1;
  return `scriptum-livePreview-mermaid-${mermaidRenderSequence}`;
}

function activeLineFromState(state: EditorState): number {
  return state.doc.lineAt(state.selection.main.head).number;
}

function activeSelectionLineRange(state: EditorState): {
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

function lineIsInActiveSelection(
  state: EditorState,
  lineNumber: number,
): boolean {
  const activeRange = activeSelectionLineRange(state);
  return (
    lineNumber >= activeRange.startLine && lineNumber <= activeRange.endLine
  );
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

  for (let lineNumber = 1; lineNumber <= state.doc.lines; lineNumber += 1) {
    if (lineIsInActiveSelection(state, lineNumber)) {
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
  from: number,
  to: number,
): boolean {
  const activeRange = activeSelectionLineRange(state);
  const startLine = state.doc.lineAt(from).number;
  const endAnchor = Math.max(from, to - 1);
  const endLine = state.doc.lineAt(endAnchor).number;

  return !(endLine < activeRange.startLine || startLine > activeRange.endLine);
}

function addInlineEmphasisDecorations(
  decorations: Array<{ from: number; to: number; decoration: Decoration }>,
  state: EditorState,
  node: { from: number; to: number; firstChild: unknown; lastChild: unknown },
  className: string,
): void {
  if (rangeTouchesActiveLine(state, node.from, node.to)) {
    return;
  }

  let contentFrom = node.from;
  let contentTo = node.to;

  const firstChild = node.firstChild as {
    from: number;
    to: number;
    type: { name: string };
  } | null;
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

  const lastChild = node.lastChild as {
    from: number;
    to: number;
    type: { name: string };
  } | null;
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
  const decorations: Array<{
    from: number;
    to: number;
    decoration: Decoration;
  }> = [];
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
      addInlineEmphasisDecorations(decorations, state, node, className);
    }

    let child = node.firstChild as {
      type: { name: string };
      from: number;
      to: number;
      firstChild: unknown;
      nextSibling: unknown;
      lastChild: unknown;
    } | null;
    while (child) {
      walk(child);
      child = child.nextSibling as {
        type: { name: string };
        from: number;
        to: number;
        firstChild: unknown;
        nextSibling: unknown;
        lastChild: unknown;
      } | null;
    }
  }

  walk(
    tree.topNode as unknown as {
      type: { name: string };
      from: number;
      to: number;
      firstChild: unknown;
      nextSibling: unknown;
      lastChild: unknown;
    },
  );

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
  const decorations: Array<{
    from: number;
    to: number;
    decoration: Decoration;
  }> = [];
  const tree = markdownLanguage.parser.parse(state.doc.toString());

  function walk(node: {
    type: { name: string };
    from: number;
    to: number;
    firstChild: unknown;
    nextSibling: unknown;
  }): void {
    const nodeType = node.type.name;

    if (
      nodeType === "Blockquote" &&
      !rangeTouchesActiveLine(state, node.from, node.to)
    ) {
      const lineStart = state.doc.lineAt(node.from).from;
      decorations.push({
        from: lineStart,
        to: lineStart,
        decoration: Decoration.line({
          class: "cm-livePreview-blockquote-line",
        }),
      });
    }

    if (
      nodeType === "QuoteMark" &&
      !rangeTouchesActiveLine(state, node.from, node.to)
    ) {
      decorations.push({
        from: node.from,
        to: node.to,
        decoration: Decoration.replace({
          inclusive: false,
        }),
      });
    }

    if (
      nodeType === "Task" &&
      !rangeTouchesActiveLine(state, node.from, node.to)
    ) {
      const marker = node.firstChild as {
        type: { name: string };
        from: number;
        to: number;
      } | null;
      if (marker && marker.type.name === "TaskMarker") {
        const markerText = state.doc
          .sliceString(marker.from, marker.to)
          .toLowerCase();
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
      !rangeTouchesActiveLine(state, node.from, node.to)
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

    let child = node.firstChild as {
      type: { name: string };
      from: number;
      to: number;
      firstChild: unknown;
      nextSibling: unknown;
    } | null;
    while (child) {
      walk(child);
      child = child.nextSibling as {
        type: { name: string };
        from: number;
        to: number;
        firstChild: unknown;
        nextSibling: unknown;
      } | null;
    }
  }

  walk(
    tree.topNode as unknown as {
      type: { name: string };
      from: number;
      to: number;
      firstChild: unknown;
      nextSibling: unknown;
    },
  );

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

  if (
    language !== "js" &&
    language !== "javascript" &&
    language !== "ts" &&
    language !== "typescript"
  ) {
    return escaped;
  }

  const pattern =
    /\b(const|let|var|function|return|if|else|for|while|class|new|import|from|export|type|interface|extends)\b|("(?:\\.|[^"])*"|'(?:\\.|[^'])*'|`(?:\\.|[^`])*`)|\b(\d+(?:\.\d+)?)\b/g;

  return escaped.replace(
    pattern,
    (match, keyword: string, stringLiteral: string, numberLiteral: string) => {
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
    },
  );
}

function sanitizeLanguageClass(language: string): string {
  return language.replaceAll(/[^a-z0-9_-]/gi, "-").toLowerCase();
}

function renderMathWithKatex(
  element: HTMLElement,
  expression: string,
  displayMode: boolean,
): boolean {
  const katexRenderer = getKatexRenderer();
  if (!katexRenderer) {
    return false;
  }

  try {
    katexRenderer.render(expression, element, {
      displayMode,
      throwOnError: false,
    });
    return true;
  } catch {
    return false;
  }
}

class MathWidget extends WidgetType {
  readonly kind: "math-block" | "math-inline";

  constructor(
    readonly expression: string,
    readonly displayMode: boolean,
  ) {
    super();
    this.kind = displayMode ? "math-block" : "math-inline";
  }

  override eq(other: WidgetType): boolean {
    return (
      other instanceof MathWidget &&
      other.expression === this.expression &&
      other.displayMode === this.displayMode
    );
  }

  override toDOM(): HTMLElement {
    const node = document.createElement(this.displayMode ? "div" : "span");
    node.className = this.displayMode
      ? "cm-livePreview-mathBlock"
      : "cm-livePreview-mathInline";
    node.setAttribute("aria-hidden", "true");

    if (!renderMathWithKatex(node, this.expression, this.displayMode)) {
      node.classList.add("cm-livePreview-mathFallback");
      node.textContent = this.displayMode
        ? `$$${this.expression}$$`
        : this.expression;
    }

    return node;
  }

  override ignoreEvent(): boolean {
    return true;
  }
}

function isPromiseLike(value: unknown): value is Promise<unknown> {
  return Boolean(
    value &&
      typeof value === "object" &&
      "then" in value &&
      typeof (value as { then?: unknown }).then === "function",
  );
}

function applyMermaidFallback(container: HTMLElement, source: string): void {
  container.replaceChildren();
  container.classList.add("cm-livePreview-mermaidFallback");

  const pre = document.createElement("pre");
  pre.className = "cm-livePreview-mermaidFallbackCode";
  pre.textContent = source;
  container.appendChild(pre);
}

function applyMermaidOutput(
  container: HTMLElement,
  output: MermaidRenderObject | string,
): boolean {
  if (typeof output === "string") {
    container.classList.remove("cm-livePreview-mermaidFallback");
    container.innerHTML = output;
    return true;
  }

  if (typeof output.svg !== "string") {
    return false;
  }

  container.classList.remove("cm-livePreview-mermaidFallback");
  container.innerHTML = output.svg;

  if (typeof output.bindFunctions === "function") {
    try {
      output.bindFunctions(container);
    } catch {
      // Ignore Mermaid post-render binding failures; preview SVG is still useful.
    }
  }

  return true;
}

function renderMermaidDiagram(container: HTMLElement, source: string): void {
  const mermaidRenderer = getMermaidRenderer();
  if (!mermaidRenderer) {
    applyMermaidFallback(container, source);
    return;
  }

  try {
    const renderResult = mermaidRenderer.render(nextMermaidRenderId(), source);
    if (isPromiseLike(renderResult)) {
      void renderResult
        .then((resolved) => {
          if (typeof resolved === "string") {
            applyMermaidOutput(container, resolved);
            return;
          }

          if (
            resolved &&
            typeof resolved === "object" &&
            "svg" in resolved &&
            typeof (resolved as { svg?: unknown }).svg === "string"
          ) {
            applyMermaidOutput(
              container,
              resolved as unknown as MermaidRenderObject,
            );
            return;
          }

          applyMermaidFallback(container, source);
        })
        .catch(() => {
          applyMermaidFallback(container, source);
        });
      return;
    }

    if (typeof renderResult === "string") {
      applyMermaidOutput(container, renderResult);
      return;
    }

    if (
      renderResult &&
      typeof renderResult === "object" &&
      "svg" in renderResult &&
      typeof (renderResult as { svg?: unknown }).svg === "string"
    ) {
      applyMermaidOutput(
        container,
        renderResult as unknown as MermaidRenderObject,
      );
      return;
    }

    applyMermaidFallback(container, source);
  } catch {
    applyMermaidFallback(container, source);
  }
}

class MermaidDiagramWidget extends WidgetType {
  readonly kind = "mermaid-diagram";

  constructor(readonly source: string) {
    super();
  }

  override eq(other: WidgetType): boolean {
    return (
      other instanceof MermaidDiagramWidget && other.source === this.source
    );
  }

  override toDOM(): HTMLElement {
    const wrapper = document.createElement("div");
    wrapper.className = "cm-livePreview-mermaidBlock";
    wrapper.setAttribute("aria-hidden", "true");
    renderMermaidDiagram(wrapper, this.source);
    return wrapper;
  }

  override ignoreEvent(): boolean {
    return true;
  }
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
      codeNode.classList.add(
        `cm-livePreview-code-lang-${sanitizeLanguageClass(this.language)}`,
      );
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
  const decorations: Array<{
    from: number;
    to: number;
    decoration: Decoration;
  }> = [];
  const tree = markdownLanguage.parser.parse(state.doc.toString());

  function walk(node: {
    type: { name: string };
    from: number;
    to: number;
    firstChild: unknown;
    nextSibling: unknown;
  }): void {
    if (
      node.type.name === "FencedCode" &&
      !rangeTouchesActiveLine(state, node.from, node.to)
    ) {
      let language: string | null = null;
      let code = "";

      let child = node.firstChild as {
        type: { name: string };
        from: number;
        to: number;
        firstChild: unknown;
        nextSibling: unknown;
      } | null;

      while (child) {
        if (child.type.name === "CodeInfo") {
          language = detectCodeLanguage(
            state.doc.sliceString(child.from, child.to),
          );
        } else if (child.type.name === "CodeText") {
          code = state.doc.sliceString(child.from, child.to);
        }
        child = child.nextSibling as {
          type: { name: string };
          from: number;
          to: number;
          firstChild: unknown;
          nextSibling: unknown;
        } | null;
      }

      const widget =
        language === "mermaid"
          ? new MermaidDiagramWidget(code)
          : new CodeBlockWidget(code, language);
      decorations.push({
        from: node.from,
        to: node.to,
        decoration: Decoration.replace({
          widget,
          inclusive: false,
        }),
      });
      return;
    }

    let child = node.firstChild as {
      type: { name: string };
      from: number;
      to: number;
      firstChild: unknown;
      nextSibling: unknown;
    } | null;
    while (child) {
      walk(child);
      child = child.nextSibling as {
        type: { name: string };
        from: number;
        to: number;
        firstChild: unknown;
        nextSibling: unknown;
      } | null;
    }
  }

  walk(
    tree.topNode as unknown as {
      type: { name: string };
      from: number;
      to: number;
      firstChild: unknown;
      nextSibling: unknown;
    },
  );

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

interface ParsedMathBlock {
  readonly endLine: number;
  readonly expression: string;
  readonly from: number;
  readonly nextLine: number;
  readonly startLine: number;
  readonly to: number;
}

interface InlineMathToken {
  readonly expression: string;
  readonly from: number;
  readonly to: number;
}

function parseMathBlock(
  state: EditorState,
  startLineNumber: number,
): ParsedMathBlock | null {
  const startLine = state.doc.line(startLineNumber);
  const trimmed = startLine.text.trim();
  if (!trimmed.startsWith("$$")) {
    return null;
  }

  if (trimmed !== "$$") {
    if (!trimmed.endsWith("$$") || trimmed.length <= 4) {
      return null;
    }

    const expression = trimmed.slice(2, -2).trim();
    if (expression.length === 0) {
      return null;
    }

    return {
      endLine: startLineNumber,
      expression,
      from: startLine.from,
      nextLine: startLineNumber + 1,
      startLine: startLineNumber,
      to: startLine.to,
    };
  }

  const blockLines: string[] = [];
  let lineNumber = startLineNumber + 1;
  while (lineNumber <= state.doc.lines) {
    const line = state.doc.line(lineNumber);
    if (line.text.trim() === "$$") {
      const expression = blockLines.join("\n").trim();
      if (expression.length === 0) {
        return null;
      }

      return {
        endLine: lineNumber,
        expression,
        from: startLine.from,
        nextLine: lineNumber + 1,
        startLine: startLineNumber,
        to: line.to,
      };
    }

    blockLines.push(line.text);
    lineNumber += 1;
  }

  return null;
}

function extractInlineMathTokens(
  lineText: string,
  lineFrom: number,
): InlineMathToken[] {
  const tokens: InlineMathToken[] = [];
  let index = 0;

  while (index < lineText.length) {
    if (
      lineText[index] !== "$" ||
      isEscapedAt(lineText, index) ||
      lineText[index + 1] === "$"
    ) {
      index += 1;
      continue;
    }

    let end = index + 1;
    while (end < lineText.length) {
      if (
        lineText[end] === "$" &&
        !isEscapedAt(lineText, end) &&
        lineText[end + 1] !== "$"
      ) {
        break;
      }
      end += 1;
    }

    if (end >= lineText.length) {
      index += 1;
      continue;
    }

    const expression = lineText.slice(index + 1, end).trim();
    if (expression.length > 0) {
      tokens.push({
        expression,
        from: lineFrom + index,
        to: lineFrom + end + 1,
      });
    }
    index = end + 1;
  }

  return tokens;
}

function collectMathBlocks(state: EditorState): ParsedMathBlock[] {
  const blocks: ParsedMathBlock[] = [];
  let inFence = false;

  for (let lineNumber = 1; lineNumber <= state.doc.lines; ) {
    const line = state.doc.line(lineNumber);
    if (isFenceDelimiter(line.text)) {
      inFence = !inFence;
      lineNumber += 1;
      continue;
    }

    if (inFence) {
      lineNumber += 1;
      continue;
    }

    const block = parseMathBlock(state, lineNumber);
    if (!block) {
      lineNumber += 1;
      continue;
    }

    blocks.push(block);
    lineNumber = block.nextLine;
  }

  return blocks;
}

function buildMathDecorations(state: EditorState): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  const mathBlocks = collectMathBlocks(state);
  const blockLines = new Set<number>();

  for (const block of mathBlocks) {
    for (
      let lineNumber = block.startLine;
      lineNumber <= block.endLine;
      lineNumber += 1
    ) {
      blockLines.add(lineNumber);
    }

    if (rangeTouchesActiveLine(state, block.from, block.to)) {
      continue;
    }

    builder.add(
      block.from,
      block.to,
      Decoration.replace({
        block: true,
        inclusive: false,
        widget: new MathWidget(block.expression, true),
      }),
    );
  }

  let inFence = false;
  for (let lineNumber = 1; lineNumber <= state.doc.lines; lineNumber += 1) {
    const line = state.doc.line(lineNumber);
    if (isFenceDelimiter(line.text)) {
      inFence = !inFence;
      continue;
    }

    if (
      inFence ||
      blockLines.has(lineNumber) ||
      lineIsInActiveSelection(state, lineNumber)
    ) {
      continue;
    }

    const tokens = extractInlineMathTokens(line.text, line.from);
    for (const token of tokens) {
      builder.add(
        token.from,
        token.to,
        Decoration.replace({
          inclusive: false,
          widget: new MathWidget(token.expression, false),
        }),
      );
    }
  }

  return builder.finish();
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

const codeBlockTheme = EditorView.baseTheme({
  ".cm-livePreview-codeBlock": {
    backgroundColor: "#0f172a",
    border: "1px solid #1e293b",
    borderRadius: "0.45rem",
    color: "#e2e8f0",
    display: "block",
    margin: "0.45em 0",
    overflowX: "auto",
    padding: "0.6rem 0.7rem",
  },
  ".cm-livePreview-codeLanguage": {
    color: "#94a3b8",
    display: "inline-block",
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
    fontSize: "0.72em",
    marginBottom: "0.4rem",
    textTransform: "lowercase",
  },
  ".cm-livePreview-codePre": {
    margin: "0",
    whiteSpace: "pre",
  },
  ".cm-livePreview-code": {
    display: "block",
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
    fontSize: "0.86em",
    lineHeight: "1.45",
  },
  ".cm-livePreview-codeToken-keyword": {
    color: "#93c5fd",
    fontWeight: "600",
  },
  ".cm-livePreview-codeToken-string": {
    color: "#86efac",
  },
  ".cm-livePreview-codeToken-number": {
    color: "#fca5a5",
  },
});

const mathPreviewTheme = EditorView.baseTheme({
  ".cm-livePreview-mathInline": {
    backgroundColor: "#f8fafc",
    border: "1px solid #dbeafe",
    borderRadius: "0.3rem",
    color: "#1d4ed8",
    display: "inline-block",
    padding: "0 0.28rem",
    verticalAlign: "baseline",
  },
  ".cm-livePreview-mathBlock": {
    backgroundColor: "#f8fafc",
    border: "1px solid #dbeafe",
    borderRadius: "0.45rem",
    color: "#1d4ed8",
    display: "block",
    margin: "0.45rem 0",
    overflowX: "auto",
    padding: "0.55rem 0.7rem",
  },
  ".cm-livePreview-mathFallback": {
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
    whiteSpace: "pre-wrap",
  },
});

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

export const codeBlockDecorations = StateField.define<DecorationSet>({
  create: buildCodeBlockDecorations,
  update(currentDecorations, transaction) {
    if (!transaction.docChanged && !transaction.selection) {
      return currentDecorations;
    }

    return buildCodeBlockDecorations(transaction.state);
  },
  provide: (field) => EditorView.decorations.from(field),
});

export const mathPreviewDecorations = StateField.define<DecorationSet>({
  create: buildMathDecorations,
  update(currentDecorations, transaction) {
    if (!transaction.docChanged && !transaction.selection) {
      return currentDecorations;
    }

    return buildMathDecorations(transaction.state);
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
  return lineIsInActiveSelection(state, lineNumber);
}

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
export const analyzeMarkdownTree = getMarkdownNodes;
export const livePreviewExtension = livePreview;
export const parseHeadingLevel = headingLevelFromLine;
