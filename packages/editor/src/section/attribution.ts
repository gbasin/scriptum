// Per-section attribution indicators showing who last edited each section
// and contribution breakdowns. Renders inline badges at section headings
// with agent/human distinction and hover tooltips.

import {
  StateEffect,
  StateField,
  type EditorState,
  type Extension,
} from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  WidgetType,
} from "@codemirror/view";

// ── Types ────────────────────────────────────────────────────────────

export type EditorType = "human" | "agent";

export interface SectionContributor {
  /** Editor display name. */
  readonly name: string;
  /** Whether this is a human or an agent. */
  readonly type: EditorType;
  /** Number of characters contributed by this editor in this section. */
  readonly charCount: number;
}

export interface SectionAttribution {
  /** 1-based line number of the section heading. */
  readonly headingLine: number;
  /** Name of the last editor. */
  readonly lastEditedBy: string;
  /** Type of the last editor. */
  readonly lastEditorType: EditorType;
  /** CSS color for the last editor badge. */
  readonly color: string;
  /** ISO timestamp of the last edit. */
  readonly lastEditedAt?: string;
  /** Per-editor contribution breakdown. */
  readonly contributors: readonly SectionContributor[];
}

// ── State effect ─────────────────────────────────────────────────────

export const setAttributions =
  StateEffect.define<readonly SectionAttribution[]>();

// ── Widget ───────────────────────────────────────────────────────────

export class AttributionBadgeWidget extends WidgetType {
  constructor(
    readonly name: string,
    readonly editorType: EditorType,
    readonly color: string,
    readonly lastEditedAt: string | undefined,
    readonly contributors: readonly SectionContributor[],
  ) {
    super();
  }

  eq(other: AttributionBadgeWidget): boolean {
    return (
      this.name === other.name &&
      this.editorType === other.editorType &&
      this.color === other.color &&
      this.lastEditedAt === other.lastEditedAt &&
      this.contributors.length === other.contributors.length &&
      this.contributors.every(
        (c, i) =>
          c.name === other.contributors[i].name &&
          c.type === other.contributors[i].type &&
          c.charCount === other.contributors[i].charCount,
      )
    );
  }

  toDOM(): HTMLElement {
    const badge = document.createElement("span");
    badge.className = "cm-attribution-badge";
    if (this.editorType === "agent") {
      badge.classList.add("cm-attribution-agent");
    }
    badge.style.borderColor = this.color;

    const icon = this.editorType === "agent" ? "\u2699 " : "";
    badge.textContent = `${icon}${this.name}`;
    badge.setAttribute(
      "aria-label",
      `Last edited by ${this.name} (${this.editorType})`,
    );

    badge.title = this.buildTooltip();

    return badge;
  }

  ignoreEvent(): boolean {
    return true;
  }

  private buildTooltip(): string {
    const lines: string[] = [];
    lines.push(
      `Last edited by ${this.name} (${this.editorType})`,
    );
    if (this.lastEditedAt) {
      lines.push(`at ${this.lastEditedAt}`);
    }

    if (this.contributors.length > 0) {
      lines.push("");
      lines.push("Contributions:");
      const totalChars = this.contributors.reduce(
        (sum, c) => sum + c.charCount,
        0,
      );
      for (const contributor of this.contributors) {
        const pct =
          totalChars > 0
            ? Math.round((contributor.charCount / totalChars) * 100)
            : 0;
        const typeLabel = contributor.type === "agent" ? " [agent]" : "";
        lines.push(
          `  ${contributor.name}${typeLabel}: ${contributor.charCount} chars (${pct}%)`,
        );
      }
    }

    return lines.join("\n");
  }
}

// ── State field ──────────────────────────────────────────────────────

interface AttributionState {
  readonly attributions: readonly SectionAttribution[];
  readonly decorations: DecorationSet;
}

export const attributionState = StateField.define<AttributionState>({
  create() {
    return { attributions: [], decorations: Decoration.none };
  },
  provide: (field) =>
    EditorView.decorations.from(field, (value) => value.decorations),
  update(current, transaction) {
    let next = current.attributions;
    let changed = false;

    for (const effect of transaction.effects) {
      if (effect.is(setAttributions)) {
        next = effect.value;
        changed = true;
      }
    }

    if (!changed && !transaction.docChanged) {
      return current;
    }

    const decorations = buildDecorations(transaction.state, next);
    if (!changed && decorations.eq(current.decorations)) {
      return current;
    }

    return { attributions: next, decorations };
  },
});

// ── Extension ────────────────────────────────────────────────────────

export function attributionExtension(): Extension {
  return [attributionState, attributionTheme];
}

// ── Decoration builder ───────────────────────────────────────────────

function buildDecorations(
  state: EditorState,
  attributions: readonly SectionAttribution[],
): DecorationSet {
  if (attributions.length === 0) {
    return Decoration.none;
  }

  const maxLine = state.doc.lines;
  const decorations: { from: number; decoration: Decoration }[] = [];

  for (const attr of attributions) {
    const line = attr.headingLine;
    if (line < 1 || line > maxLine || !Number.isFinite(line)) {
      continue;
    }

    const docLine = state.doc.line(line);
    decorations.push({
      from: docLine.to,
      decoration: Decoration.widget({
        widget: new AttributionBadgeWidget(
          attr.lastEditedBy,
          attr.lastEditorType,
          attr.color,
          attr.lastEditedAt,
          attr.contributors,
        ),
        side: 1,
      }),
    });
  }

  decorations.sort((a, b) => a.from - b.from);
  return Decoration.set(
    decorations.map(({ from, decoration }) => decoration.range(from)),
  );
}

// ── Theme ────────────────────────────────────────────────────────────

const attributionTheme = EditorView.baseTheme({
  ".cm-attribution-badge": {
    display: "inline-block",
    fontSize: "0.65em",
    lineHeight: "1.2",
    padding: "1px 5px",
    marginLeft: "8px",
    borderRadius: "3px",
    borderWidth: "1.5px",
    borderStyle: "solid",
    color: "#555",
    backgroundColor: "#f5f5f5",
    fontFamily: "sans-serif",
    fontWeight: "500",
    whiteSpace: "nowrap",
    verticalAlign: "middle",
    opacity: "0.85",
    cursor: "default",
  },
  ".cm-attribution-agent": {
    backgroundColor: "#eef0ff",
    color: "#3b4c9b",
    fontStyle: "italic",
  },
});
