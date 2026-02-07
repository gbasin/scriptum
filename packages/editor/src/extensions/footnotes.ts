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

const FOOTNOTE_REFERENCE_PATTERN = /\[\^([^\]\s]+)\]/g;
const FOOTNOTE_DEFINITION_PATTERN = /^\[\^([^\]\s]+)\]:\s*(.*)$/;

interface ParsedFootnoteDefinition {
  readonly content: string;
  readonly endLine: number;
  readonly from: number;
  readonly id: string;
  readonly number: number;
  readonly startLine: number;
  readonly to: number;
}

interface FootnoteSectionEntry {
  readonly content: string;
  readonly id: string;
  readonly number: number;
}

function activeSelectionLineRange(
  state: EditorState,
): { readonly endLine: number; readonly startLine: number } {
  const from = state.selection.main.from;
  const to = state.selection.main.to;
  const startLine = state.doc.lineAt(Math.min(from, to)).number;
  const endAnchor = Math.max(Math.min(from, to), Math.max(from, to) - 1);
  const endLine = state.doc.lineAt(endAnchor).number;
  return { endLine, startLine };
}

function lineIsInActiveSelection(state: EditorState, lineNumber: number): boolean {
  const activeRange = activeSelectionLineRange(state);
  return lineNumber >= activeRange.startLine && lineNumber <= activeRange.endLine;
}

function lineLooksLikeFootnoteContinuation(lineText: string): boolean {
  return /^(?: {4}|\t)/.test(lineText);
}

function parseFootnoteDefinitions(state: EditorState): ParsedFootnoteDefinition[] {
  const definitions: Omit<ParsedFootnoteDefinition, "number">[] = [];
  let lineNumber = 1;

  while (lineNumber <= state.doc.lines) {
    const line = state.doc.line(lineNumber);
    const match = FOOTNOTE_DEFINITION_PATTERN.exec(line.text);
    if (!match) {
      lineNumber += 1;
      continue;
    }

    const id = match[1] ?? "";
    const contentLines = [match[2] ?? ""];
    let endLine = lineNumber;

    while (endLine + 1 <= state.doc.lines) {
      const continuation = state.doc.line(endLine + 1);
      if (!lineLooksLikeFootnoteContinuation(continuation.text)) {
        break;
      }
      contentLines.push(continuation.text.replace(/^(?: {4}|\t)/, ""));
      endLine += 1;
    }

    definitions.push({
      content: contentLines.join("\n").trimEnd(),
      endLine,
      from: line.from,
      id,
      startLine: lineNumber,
      to: state.doc.line(endLine).to,
    });
    lineNumber = endLine + 1;
  }

  const numberById = new Map<string, number>();
  let nextNumber = 1;

  return definitions.map((definition) => {
    let number = numberById.get(definition.id);
    if (!number) {
      number = nextNumber;
      numberById.set(definition.id, number);
      nextNumber += 1;
    }
    return { ...definition, number };
  });
}

class FootnoteReferenceWidget extends WidgetType {
  readonly kind = "footnote-reference";

  constructor(
    readonly id: string,
    readonly number: number | null,
  ) {
    super();
  }

  eq(other: WidgetType): boolean {
    return (
      other instanceof FootnoteReferenceWidget &&
      other.id === this.id &&
      other.number === this.number
    );
  }

  toDOM(): HTMLElement {
    const sup = document.createElement("sup");
    sup.className = "cm-livePreview-footnoteRef";
    sup.textContent = this.number ? `[${this.number}]` : `[^${this.id}]`;
    return sup;
  }
}

class FootnoteSectionWidget extends WidgetType {
  readonly kind = "footnote-section";

  constructor(readonly entries: readonly FootnoteSectionEntry[]) {
    super();
  }

  eq(other: WidgetType): boolean {
    return (
      other instanceof FootnoteSectionWidget &&
      JSON.stringify(other.entries) === JSON.stringify(this.entries)
    );
  }

  toDOM(): HTMLElement {
    const section = document.createElement("section");
    section.className = "cm-livePreview-footnoteSection";

    const label = document.createElement("div");
    label.className = "cm-livePreview-footnoteSectionLabel";
    label.textContent = "Footnotes";
    section.appendChild(label);

    const list = document.createElement("ol");
    list.className = "cm-livePreview-footnoteList";

    this.entries.forEach((entry) => {
      const item = document.createElement("li");
      item.className = "cm-livePreview-footnoteItem";
      item.dataset.footnoteId = entry.id;

      const marker = document.createElement("span");
      marker.className = "cm-livePreview-footnoteMarker";
      marker.textContent = `${entry.number}.`;
      item.appendChild(marker);

      const content = document.createElement("span");
      content.className = "cm-livePreview-footnoteContent";
      content.textContent = entry.content || " ";
      item.appendChild(content);

      list.appendChild(item);
    });

    section.appendChild(list);
    return section;
  }
}

function buildFootnoteDecorations(state: EditorState): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  const definitions = parseFootnoteDefinitions(state);
  const numberById = new Map<string, number>();
  const uniqueDefinitions: FootnoteSectionEntry[] = [];
  const definitionStartLines = new Set<number>();

  definitions.forEach((definition) => {
    definitionStartLines.add(definition.startLine);
    numberById.set(definition.id, definition.number);
    if (!uniqueDefinitions.some((entry) => entry.id === definition.id)) {
      uniqueDefinitions.push({
        content: definition.content,
        id: definition.id,
        number: definition.number,
      });
    }
  });

  for (let lineNumber = 1; lineNumber <= state.doc.lines; lineNumber += 1) {
    if (lineIsInActiveSelection(state, lineNumber)) {
      continue;
    }

    if (definitionStartLines.has(lineNumber)) {
      continue;
    }

    const line = state.doc.line(lineNumber);
    for (const match of line.text.matchAll(FOOTNOTE_REFERENCE_PATTERN)) {
      const fullMatch = match[0] ?? "";
      const matchIndex = match.index ?? -1;
      if (matchIndex < 0 || fullMatch.length === 0) {
        continue;
      }

      const nextCharacter = line.text[matchIndex + fullMatch.length];
      if (nextCharacter === ":") {
        continue;
      }

      const id = match[1] ?? "";
      builder.add(
        line.from + matchIndex,
        line.from + matchIndex + fullMatch.length,
        Decoration.replace({
          inclusive: false,
          widget: new FootnoteReferenceWidget(id, numberById.get(id) ?? null),
        }),
      );
    }
  }

  const activeRange = activeSelectionLineRange(state);
  const activeTouchesDefinition = definitions.some(
    (definition) =>
      definition.endLine >= activeRange.startLine &&
      definition.startLine <= activeRange.endLine,
  );

  if (!activeTouchesDefinition) {
    definitions.forEach((definition) => {
      builder.add(
        definition.from,
        definition.to,
        Decoration.replace({
          block: true,
          inclusive: false,
        }),
      );
    });

    if (uniqueDefinitions.length > 0) {
      builder.add(
        state.doc.length,
        state.doc.length,
        Decoration.widget({
          block: true,
          side: 1,
          widget: new FootnoteSectionWidget(uniqueDefinitions),
        }),
      );
    }
  }

  return builder.finish();
}

const footnotePreviewTheme = EditorView.baseTheme({
  ".cm-livePreview-footnoteRef": {
    color: "#1d4ed8",
    fontSize: "0.72em",
    fontWeight: "650",
    verticalAlign: "super",
  },
  ".cm-livePreview-footnoteSection": {
    borderTop: "1px solid #dbeafe",
    color: "#334155",
    marginTop: "0.8rem",
    paddingTop: "0.5rem",
  },
  ".cm-livePreview-footnoteSectionLabel": {
    color: "#1e3a8a",
    fontSize: "0.8em",
    fontWeight: "700",
    letterSpacing: "0.02em",
    marginBottom: "0.3rem",
    textTransform: "uppercase",
  },
  ".cm-livePreview-footnoteList": {
    margin: "0",
    paddingLeft: "1.1rem",
  },
  ".cm-livePreview-footnoteItem": {
    margin: "0.2rem 0",
  },
  ".cm-livePreview-footnoteMarker": {
    color: "#1e3a8a",
    display: "inline-block",
    fontWeight: "650",
    marginRight: "0.35rem",
    minWidth: "1.4rem",
  },
  ".cm-livePreview-footnoteContent": {
    whiteSpace: "pre-wrap",
  },
});

export const footnotePreviewDecorations = StateField.define<DecorationSet>({
  create: buildFootnoteDecorations,
  update(currentDecorations, transaction) {
    if (!transaction.docChanged && !transaction.selection) {
      return currentDecorations;
    }

    return buildFootnoteDecorations(transaction.state);
  },
  provide: (field) => EditorView.decorations.from(field),
});

export function footnotePreview(): Extension {
  return [footnotePreviewDecorations, footnotePreviewTheme];
}

export const footnotePreviewExtension = footnotePreview;
