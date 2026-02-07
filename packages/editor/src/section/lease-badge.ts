// Inline lease badges rendered next to section headings.
// Shows "[agent-name editing]" when an agent holds an advisory lease
// on a section. Badge color matches the agent's cursor color.

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

export interface LeaseBadgeData {
  /** Agent or user display name. */
  readonly agentName: string;
  /** CSS color for the badge (use nameToColor for consistency). */
  readonly color: string;
  /** 1-based line number of the heading this lease applies to. */
  readonly headingLine: number;
  /** Optional: when the lease expires (ms since epoch). */
  readonly expiresAt?: number;
}

// ── State effect ─────────────────────────────────────────────────────

export const setLeases = StateEffect.define<readonly LeaseBadgeData[]>();

// ── Widget ───────────────────────────────────────────────────────────

export class LeaseBadgeWidget extends WidgetType {
  constructor(
    readonly agentName: string,
    readonly color: string,
    readonly expiresAt: number | undefined,
  ) {
    super();
  }

  eq(other: LeaseBadgeWidget): boolean {
    return (
      this.agentName === other.agentName &&
      this.color === other.color &&
      this.expiresAt === other.expiresAt
    );
  }

  toDOM(): HTMLElement {
    const badge = document.createElement("span");
    badge.className = "cm-lease-badge";
    badge.style.backgroundColor = this.color;
    badge.textContent = `${this.agentName} editing`;
    badge.setAttribute(
      "aria-label",
      `${this.agentName} is editing this section`,
    );

    if (this.expiresAt != null) {
      const remaining = Math.max(0, this.expiresAt - Date.now());
      const minutes = Math.ceil(remaining / 60_000);
      badge.title = `${this.agentName} editing · expires in ~${minutes}m`;
    } else {
      badge.title = `${this.agentName} editing`;
    }

    return badge;
  }

  ignoreEvent(): boolean {
    return true;
  }
}

// ── State field ──────────────────────────────────────────────────────

interface LeaseBadgeState {
  readonly leases: readonly LeaseBadgeData[];
  readonly decorations: DecorationSet;
}

export const leaseBadgeState = StateField.define<LeaseBadgeState>({
  create() {
    return { leases: [], decorations: Decoration.none };
  },
  provide: (field) =>
    EditorView.decorations.from(field, (value) => value.decorations),
  update(current, transaction) {
    let nextLeases = current.leases;
    let leasesChanged = false;

    for (const effect of transaction.effects) {
      if (effect.is(setLeases)) {
        nextLeases = effect.value;
        leasesChanged = true;
      }
    }

    if (!leasesChanged && !transaction.docChanged) {
      return current;
    }

    const decorations = buildDecorations(transaction.state, nextLeases);
    if (!leasesChanged && RangeSet.eq([decorations], [current.decorations])) {
      return current;
    }

    return { leases: nextLeases, decorations };
  },
});

// ── Extension ────────────────────────────────────────────────────────

export function leaseBadgeExtension(): Extension {
  return [leaseBadgeState, leaseBadgeTheme];
}

// ── Decoration builder ───────────────────────────────────────────────

function buildDecorations(
  state: EditorState,
  leases: readonly LeaseBadgeData[],
): DecorationSet {
  if (leases.length === 0) {
    return Decoration.none;
  }

  const maxLine = state.doc.lines;
  const decorations: { from: number; decoration: Decoration }[] = [];

  for (const lease of leases) {
    const line = lease.headingLine;
    if (line < 1 || line > maxLine || !Number.isFinite(line)) {
      continue;
    }

    const docLine = state.doc.line(line);
    decorations.push({
      from: docLine.to,
      decoration: Decoration.widget({
        widget: new LeaseBadgeWidget(
          lease.agentName,
          lease.color,
          lease.expiresAt,
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

const leaseBadgeTheme = EditorView.baseTheme({
  ".cm-lease-badge": {
    display: "inline-block",
    fontSize: "0.7em",
    lineHeight: "1.2",
    padding: "1px 6px",
    marginLeft: "8px",
    borderRadius: "3px",
    color: "white",
    fontFamily: "sans-serif",
    fontWeight: "500",
    whiteSpace: "nowrap",
    verticalAlign: "middle",
    opacity: "0.9",
    cursor: "default",
  },
});
