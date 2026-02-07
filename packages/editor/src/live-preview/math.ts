import {
  type EditorState,
  RangeSetBuilder,
  StateField,
} from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  WidgetType,
} from "@codemirror/view";
import {
  getGlobalRecord,
  isEscapedAt,
  isFenceDelimiter,
  lineIsInActiveSelection,
  rangeTouchesActiveLine,
} from "./shared";

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
  const entries: Array<{
    decoration: Decoration;
    from: number;
    to: number;
  }> = [];
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

    entries.push({
      decoration: Decoration.replace({
        block: true,
        inclusive: false,
        widget: new MathWidget(block.expression, true),
      }),
      from: block.from,
      to: block.to,
    });
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
      entries.push({
        decoration: Decoration.replace({
          inclusive: false,
          widget: new MathWidget(token.expression, false),
        }),
        from: token.from,
        to: token.to,
      });
    }
  }

  entries.sort((left, right) => {
    if (left.from !== right.from) {
      return left.from - right.from;
    }
    return left.to - right.to;
  });

  for (const entry of entries) {
    builder.add(entry.from, entry.to, entry.decoration);
  }

  return builder.finish();
}

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

export const mathPreviewTheme = EditorView.baseTheme({
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
