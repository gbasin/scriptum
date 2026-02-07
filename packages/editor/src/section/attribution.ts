// Per-section attribution indicators showing who last edited each section
// and contribution breakdowns. Renders inline badges at section headings
// with agent/human distinction and hover tooltips.

import {
  type EditorState,
  type Extension,
  RangeSet,
  StateEffect,
  StateField,
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
  /** Stable editor identifier. */
  readonly authorId: string;
  /** Name of the last editor. */
  readonly lastEditedBy: string;
  /** Type of the last editor. */
  readonly lastEditorType: EditorType;
  /** CSS color for the last editor badge. */
  readonly color: string;
  /** ISO timestamp of the last edit. */
  readonly lastEditedAt: string;
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
    readonly authorId: string,
    readonly editorType: EditorType,
    readonly color: string,
    readonly lastEditedAt: string,
    readonly contributors: readonly SectionContributor[],
  ) {
    super();
  }

  eq(other: AttributionBadgeWidget): boolean {
    return (
      this.name === other.name &&
      this.authorId === other.authorId &&
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
    } else {
      badge.classList.add("cm-attribution-human");
    }
    badge.style.borderColor = this.color;

    const relativeEditedAt = formatRelativeTimestamp(this.lastEditedAt);
    badge.textContent = buildAttributionBadgeText(
      this.name,
      this.editorType,
      this.lastEditedAt,
    );
    badge.setAttribute(
      "aria-label",
      `Last edited by ${this.name} (${this.editorType}) ${relativeEditedAt}`,
    );

    badge.title = this.buildTooltip();

    return badge;
  }

  ignoreEvent(): boolean {
    return true;
  }

  private buildTooltip(): string {
    const lines: string[] = [];
    lines.push(`Last edited by ${this.name} (${this.editorType})`);
    lines.push(`Author ID: ${this.authorId}`);
    lines.push(
      `${formatRelativeTimestamp(this.lastEditedAt)} (${this.lastEditedAt})`,
    );

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

export function formatRelativeTimestamp(
  isoTimestamp: string,
  nowMs = Date.now(),
): string {
  const parsedMs = Date.parse(isoTimestamp);
  if (Number.isNaN(parsedMs)) {
    return isoTimestamp;
  }

  const deltaMs = nowMs - parsedMs;
  const isFuture = deltaMs < 0;
  const absMs = Math.abs(deltaMs);

  if (absMs < 60_000) {
    return isFuture ? "in <1m" : "just now";
  }

  const minutes = Math.floor(absMs / 60_000);
  if (minutes < 60) {
    return isFuture ? `in ${minutes}m` : `${minutes}m ago`;
  }

  const hours = Math.floor(absMs / 3_600_000);
  if (hours < 24) {
    return isFuture ? `in ${hours}h` : `${hours}h ago`;
  }

  const days = Math.floor(absMs / 86_400_000);
  if (days < 30) {
    return isFuture ? `in ${days}d` : `${days}d ago`;
  }

  const months = Math.floor(days / 30);
  if (months < 12) {
    return isFuture ? `in ${months}mo` : `${months}mo ago`;
  }

  const years = Math.floor(days / 365);
  return isFuture ? `in ${years}y` : `${years}y ago`;
}

export function buildAttributionBadgeText(
  name: string,
  editorType: EditorType,
  isoTimestamp: string,
  nowMs = Date.now(),
): string {
  const icon = editorType === "agent" ? "\u2699" : "\u{1F464}";
  const relativeEditedAt = formatRelativeTimestamp(isoTimestamp, nowMs);
  return `${icon} ${name} · ${relativeEditedAt}`;
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
    if (!changed && RangeSet.eq([decorations], [current.decorations])) {
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
          attr.authorId,
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
  ".cm-attribution-human": {
    backgroundColor: "#f5f5f5",
    color: "#555",
  },
});
