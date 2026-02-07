import { type EditorState, RangeSetBuilder, StateField } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, WidgetType } from "@codemirror/view";
import { rangeTouchesActiveLine } from "./shared";

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
  const trimmed = lineText.trim();
  if (!trimmed.startsWith("|") || !trimmed.endsWith("|")) {
    return null;
  }

  const cells = trimmed
    .slice(1, -1)
    .split("|")
    .map((cell) => cell.trim());
  if (cells.length === 0) {
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
    const trimmed = cell.trim();
    if (!/^:?-{3,}:?$/.test(trimmed)) {
      return null;
    }

    const hasLeft = trimmed.startsWith(":");
    const hasRight = trimmed.endsWith(":");
    if (hasLeft && hasRight) {
      alignments.push("center");
    } else if (hasRight) {
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
  if (startLineNumber >= state.doc.lines) {
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

  override eq(other: WidgetType): boolean {
    return (
      other instanceof InlineTableWidget &&
      JSON.stringify(other.headers) === JSON.stringify(this.headers) &&
      JSON.stringify(other.rows) === JSON.stringify(this.rows) &&
      JSON.stringify(other.alignments) === JSON.stringify(this.alignments)
    );
  }

  override toDOM(): HTMLElement {
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

export const tablePreviewTheme = EditorView.baseTheme({
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
