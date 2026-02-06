import {
  StateEffect,
  StateField,
  type EditorState,
  type Extension,
} from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView } from "@codemirror/view";

export type SectionOverlapSeverity = "info" | "warning";

export interface SectionOverlapSection {
  readonly id: string;
  readonly startLine?: number;
  readonly endLine?: number;
  readonly start_line?: number;
  readonly end_line?: number;
}

export interface SectionOverlapData {
  readonly section: SectionOverlapSection;
  readonly severity: SectionOverlapSeverity | string;
}

interface NormalizedSectionOverlap {
  readonly sectionId: string;
  readonly startLine: number;
  readonly endLine: number;
  readonly severity: SectionOverlapSeverity;
}

interface SectionOverlapIndicatorState {
  readonly overlaps: readonly NormalizedSectionOverlap[];
  readonly decorations: DecorationSet;
}

export const setSectionOverlaps = StateEffect.define<
  readonly SectionOverlapData[]
>();

export const sectionOverlapIndicatorState = StateField.define<SectionOverlapIndicatorState>(
  {
    create() {
      return {
        overlaps: [],
        decorations: Decoration.none,
      };
    },
    provide: (field) =>
      EditorView.decorations.from(field, (value) => value.decorations),
    update(current, transaction) {
      let nextOverlaps = current.overlaps;
      let overlapsChanged = false;

      for (const effect of transaction.effects) {
        if (effect.is(setSectionOverlaps)) {
          nextOverlaps = normalizeOverlaps(transaction.state, effect.value);
          overlapsChanged = true;
        }
      }

      if (!overlapsChanged && !transaction.docChanged) {
        return current;
      }

      const decorations = buildDecorations(transaction.state, nextOverlaps);
      if (!overlapsChanged && decorations.eq(current.decorations)) {
        return current;
      }

      return {
        overlaps: nextOverlaps,
        decorations,
      };
    },
  },
);

const sectionOverlapTheme = EditorView.baseTheme({
  ".cm-sectionOverlap": {
    boxSizing: "border-box",
    borderLeft: "3px solid transparent",
    paddingLeft: "0.25rem",
  },
  ".cm-sectionOverlap-info": {
    borderLeftColor: "rgba(14, 165, 233, 0.85)",
    backgroundColor: "rgba(14, 165, 233, 0.12)",
  },
  ".cm-sectionOverlap-warning": {
    borderLeftColor: "rgba(245, 158, 11, 0.95)",
    backgroundColor: "rgba(245, 158, 11, 0.18)",
  },
});

export function overlapIndicatorExtension(): Extension {
  return [sectionOverlapIndicatorState, sectionOverlapTheme];
}

function normalizeOverlaps(
  state: EditorState,
  overlaps: readonly SectionOverlapData[],
): readonly NormalizedSectionOverlap[] {
  if (overlaps.length === 0) {
    return [];
  }

  const maxLine = Math.max(state.doc.lines, 1);
  const bySection = new Map<string, NormalizedSectionOverlap>();

  for (const overlap of overlaps) {
    const sectionId = overlap.section.id.trim();
    if (sectionId.length === 0) {
      continue;
    }

    const startLine = clampLine(
      overlap.section.startLine ?? overlap.section.start_line ?? 1,
      maxLine,
    );
    const endLine = clampLine(
      overlap.section.endLine ?? overlap.section.end_line ?? startLine,
      maxLine,
    );
    const normalized: NormalizedSectionOverlap = {
      sectionId,
      startLine: Math.min(startLine, endLine),
      endLine: Math.max(startLine, endLine),
      severity: normalizeSeverity(overlap.severity),
    };

    const existing = bySection.get(sectionId);
    if (!existing) {
      bySection.set(sectionId, normalized);
      continue;
    }

    bySection.set(sectionId, {
      sectionId,
      startLine: Math.min(existing.startLine, normalized.startLine),
      endLine: Math.max(existing.endLine, normalized.endLine),
      severity:
        existing.severity === "warning" || normalized.severity === "warning"
          ? "warning"
          : "info",
    });
  }

  return Array.from(bySection.values()).sort((left, right) => {
    if (left.startLine !== right.startLine) {
      return left.startLine - right.startLine;
    }
    return left.sectionId.localeCompare(right.sectionId);
  });
}

function normalizeSeverity(value: string): SectionOverlapSeverity {
  return value === "warning" ? "warning" : "info";
}

function clampLine(lineNumber: number, maxLine: number): number {
  if (!Number.isFinite(lineNumber)) {
    return 1;
  }
  const rounded = Math.floor(lineNumber);
  if (rounded < 1) {
    return 1;
  }
  if (rounded > maxLine) {
    return maxLine;
  }
  return rounded;
}

function buildDecorations(
  state: EditorState,
  overlaps: readonly NormalizedSectionOverlap[],
): DecorationSet {
  if (overlaps.length === 0) {
    return Decoration.none;
  }

  const decorations = overlaps.map((overlap) => {
    const line = state.doc.line(overlap.startLine);
    return Decoration.line({
      attributes: {
        "data-section-overlap": overlap.sectionId,
        "data-severity": overlap.severity,
      },
      class: `cm-sectionOverlap cm-sectionOverlap-${overlap.severity}`,
    }).range(line.from);
  });

  return Decoration.set(decorations, true);
}
