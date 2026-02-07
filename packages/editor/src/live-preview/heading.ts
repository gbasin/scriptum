import {
  type EditorState,
  RangeSetBuilder,
  StateField,
} from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView } from "@codemirror/view";
import { lineIsInActiveSelection } from "./shared";

const HEADING_PATTERN = /^(#{1,6})(\s+)(.*)$/;

export function headingLevelFromLine(text: string): number | null {
  const match = HEADING_PATTERN.exec(text);
  if (!match) {
    return null;
  }

  return match[1].length;
}

function buildHeadingDecorations(state: EditorState): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();

  for (let lineNumber = 1; lineNumber <= state.doc.lines; lineNumber += 1) {
    if (lineIsInActiveSelection(state, lineNumber)) {
      continue;
    }

    const line = state.doc.line(lineNumber);
    const headingLevel = headingLevelFromLine(line.text);
    if (!headingLevel) {
      continue;
    }

    const match = HEADING_PATTERN.exec(line.text);
    if (!match) {
      continue;
    }

    const markerLength = match[1].length + match[2].length;
    const contentStart = line.from + markerLength;

    builder.add(
      line.from,
      line.from,
      Decoration.line({
        class: `cm-livePreview-heading-line cm-livePreview-heading-line-h${headingLevel}`,
      }),
    );

    if (markerLength > 0) {
      builder.add(
        line.from,
        contentStart,
        Decoration.replace({
          inclusive: false,
        }),
      );
    }

    if (contentStart < line.to) {
      builder.add(
        contentStart,
        line.to,
        Decoration.mark({
          class: `cm-livePreview-heading cm-livePreview-heading-h${headingLevel}`,
        }),
      );
    }
  }

  return builder.finish();
}

export const headingPreviewDecorations = StateField.define<DecorationSet>({
  create: buildHeadingDecorations,
  update(currentDecorations, transaction) {
    if (!transaction.docChanged && !transaction.selection) {
      return currentDecorations;
    }

    return buildHeadingDecorations(transaction.state);
  },
  provide: (field) => EditorView.decorations.from(field),
});

export const headingPreviewTheme = EditorView.baseTheme({
  ".cm-livePreview-heading": {
    fontWeight: "600",
    letterSpacing: "-0.01em",
  },
  ".cm-livePreview-heading-h1": {
    fontSize: "1.9em",
    fontWeight: "700",
    lineHeight: "1.15",
  },
  ".cm-livePreview-heading-h2": {
    fontSize: "1.55em",
    fontWeight: "680",
    lineHeight: "1.2",
  },
  ".cm-livePreview-heading-h3": {
    fontSize: "1.35em",
    lineHeight: "1.25",
  },
  ".cm-livePreview-heading-h4": {
    fontSize: "1.2em",
    lineHeight: "1.3",
  },
  ".cm-livePreview-heading-h5": {
    fontSize: "1.05em",
    lineHeight: "1.35",
  },
  ".cm-livePreview-heading-h6": {
    fontSize: "0.95em",
    fontWeight: "650",
    lineHeight: "1.35",
    textTransform: "uppercase",
    letterSpacing: "0.03em",
  },
});
