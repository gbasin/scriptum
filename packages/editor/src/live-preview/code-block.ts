import { markdownLanguage } from "@codemirror/lang-markdown";
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
import { getGlobalRecord, rangeTouchesActiveLine } from "./shared";

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

function nextMermaidRenderId(): string {
  mermaidRenderSequence += 1;
  return `scriptum-livePreview-mermaid-${mermaidRenderSequence}`;
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

export const codeBlockTheme = EditorView.baseTheme({
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
