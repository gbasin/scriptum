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
import { lineIsInActiveSelection } from "./shared";

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

export const inlineLinkTheme = EditorView.baseTheme({
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
