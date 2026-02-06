import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";

import {
  overlapIndicatorExtension,
  sectionOverlapIndicatorState,
  setSectionOverlaps,
} from "./overlap-indicator";

interface OverlapLineDecoration {
  className: string;
  lineNumber: number;
  sectionId: string;
  severity: string;
}

describe("overlapIndicatorExtension", () => {
  it("renders info and warning overlap line decorations", () => {
    const state = EditorState.create({
      doc: "# Root\nalpha\n## Child\nbeta",
      extensions: [overlapIndicatorExtension()],
    }).update({
      effects: [
        setSectionOverlaps.of([
          {
            section: { endLine: 2, id: "root", startLine: 1 },
            severity: "info",
          },
          {
            section: { end_line: 4, id: "root/child", start_line: 3 },
            severity: "warning",
          },
        ]),
      ],
    }).state;

    expect(collectDecorations(state)).toEqual([
      {
        className: "cm-sectionOverlap cm-sectionOverlap-info",
        lineNumber: 1,
        sectionId: "root",
        severity: "info",
      },
      {
        className: "cm-sectionOverlap cm-sectionOverlap-warning",
        lineNumber: 3,
        sectionId: "root/child",
        severity: "warning",
      },
    ]);
  });

  it("deduplicates section overlaps and keeps highest severity", () => {
    const state = EditorState.create({
      doc: "# Shared\nbody",
      extensions: [overlapIndicatorExtension()],
    }).update({
      effects: [
        setSectionOverlaps.of([
          {
            section: { endLine: 2, id: "shared", startLine: 1 },
            severity: "info",
          },
          {
            section: { endLine: 2, id: "shared", startLine: 1 },
            severity: "warning",
          },
        ]),
      ],
    }).state;

    expect(collectDecorations(state)).toEqual([
      {
        className: "cm-sectionOverlap cm-sectionOverlap-warning",
        lineNumber: 1,
        sectionId: "shared",
        severity: "warning",
      },
    ]);
  });

  it("replaces previous overlaps and ignores invalid section ids", () => {
    let state = EditorState.create({
      doc: "# A\n## B\n## C",
      extensions: [overlapIndicatorExtension()],
    });

    state = state.update({
      effects: [
        setSectionOverlaps.of([
          {
            section: { id: "a", startLine: 1 },
            severity: "warning",
          },
        ]),
      ],
    }).state;
    expect(collectDecorations(state).length).toBe(1);

    state = state.update({
      effects: [
        setSectionOverlaps.of([
          {
            section: { id: "   ", startLine: 2 },
            severity: "warning",
          },
          {
            section: { id: "c", startLine: 3 },
            severity: "info",
          },
        ]),
      ],
    }).state;

    expect(collectDecorations(state)).toEqual([
      {
        className: "cm-sectionOverlap cm-sectionOverlap-info",
        lineNumber: 3,
        sectionId: "c",
        severity: "info",
      },
    ]);
  });
});

function collectDecorations(state: EditorState): OverlapLineDecoration[] {
  const decorations = state.field(sectionOverlapIndicatorState).decorations;
  const results: OverlapLineDecoration[] = [];

  decorations.between(0, state.doc.length, (from, _to, value) => {
    const spec = value.spec as {
      attributes?: Record<string, string>;
      class?: string;
    };
    const className = spec.class;
    if (!className || !className.includes("cm-sectionOverlap")) {
      return;
    }

    results.push({
      className,
      lineNumber: state.doc.lineAt(from).number,
      sectionId: spec.attributes?.["data-section-overlap"] ?? "",
      severity: spec.attributes?.["data-severity"] ?? "",
    });
  });

  results.sort((left, right) => left.lineNumber - right.lineNumber);
  return results;
}
