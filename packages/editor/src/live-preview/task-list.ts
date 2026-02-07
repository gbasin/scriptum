import { markdownLanguage } from "@codemirror/lang-markdown";
import { type EditorState, StateField } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  WidgetType,
} from "@codemirror/view";
import { rangeTouchesActiveLine } from "./shared";

class TaskCheckboxWidget extends WidgetType {
  readonly kind = "task-checkbox";

  constructor(readonly checked: boolean) {
    super();
  }

  override eq(other: TaskCheckboxWidget): boolean {
    return this.checked === other.checked;
  }

  override toDOM(): HTMLElement {
    const node = document.createElement("span");
    node.className = "cm-livePreview-task-checkbox";
    node.setAttribute("aria-hidden", "true");
    node.textContent = this.checked ? "☑" : "☐";
    return node;
  }

  override ignoreEvent(): boolean {
    return true;
  }
}

class HorizontalRuleWidget extends WidgetType {
  readonly kind = "horizontal-rule";

  override eq(): boolean {
    return true;
  }

  override toDOM(): HTMLElement {
    const node = document.createElement("span");
    node.className = "cm-livePreview-hr";
    node.setAttribute("aria-hidden", "true");
    return node;
  }

  override ignoreEvent(): boolean {
    return true;
  }
}

function buildTaskBlockquoteHrDecorations(state: EditorState): DecorationSet {
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
    const nodeType = node.type.name;

    if (
      nodeType === "Blockquote" &&
      !rangeTouchesActiveLine(state, node.from, node.to)
    ) {
      const lineStart = state.doc.lineAt(node.from).from;
      decorations.push({
        from: lineStart,
        to: lineStart,
        decoration: Decoration.line({
          class: "cm-livePreview-blockquote-line",
        }),
      });
    }

    if (
      nodeType === "QuoteMark" &&
      !rangeTouchesActiveLine(state, node.from, node.to)
    ) {
      decorations.push({
        from: node.from,
        to: node.to,
        decoration: Decoration.replace({
          inclusive: false,
        }),
      });
    }

    if (
      nodeType === "Task" &&
      !rangeTouchesActiveLine(state, node.from, node.to)
    ) {
      const marker = node.firstChild as {
        type: { name: string };
        from: number;
        to: number;
      } | null;
      if (marker && marker.type.name === "TaskMarker") {
        const markerText = state.doc
          .sliceString(marker.from, marker.to)
          .toLowerCase();
        const checked = markerText.includes("x");

        decorations.push({
          from: marker.from,
          to: marker.to,
          decoration: Decoration.replace({
            widget: new TaskCheckboxWidget(checked),
            inclusive: false,
          }),
        });

        let contentFrom = marker.to;
        if (
          contentFrom < node.to &&
          state.doc.sliceString(contentFrom, contentFrom + 1) === " "
        ) {
          contentFrom += 1;
        }

        if (contentFrom < node.to) {
          decorations.push({
            from: contentFrom,
            to: node.to,
            decoration: Decoration.mark({
              class: "cm-livePreview-task-content",
            }),
          });
        }
      }
    }

    if (
      nodeType === "HorizontalRule" &&
      !rangeTouchesActiveLine(state, node.from, node.to)
    ) {
      decorations.push({
        from: node.from,
        to: node.to,
        decoration: Decoration.replace({
          widget: new HorizontalRuleWidget(),
          inclusive: false,
        }),
      });
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

export const taskBlockquoteHrDecorations = StateField.define<DecorationSet>({
  create: buildTaskBlockquoteHrDecorations,
  update(currentDecorations, transaction) {
    if (!transaction.docChanged && !transaction.selection) {
      return currentDecorations;
    }

    return buildTaskBlockquoteHrDecorations(transaction.state);
  },
  provide: (field) => EditorView.decorations.from(field),
});

export const taskBlockquoteHrTheme = EditorView.baseTheme({
  ".cm-livePreview-blockquote-line": {
    borderLeft: "3px solid #cbd5e1",
    color: "#475569",
    paddingLeft: "0.75rem",
  },
  ".cm-livePreview-task-checkbox": {
    color: "#0f766e",
    display: "inline-block",
    marginRight: "0.35rem",
    width: "1.1em",
  },
  ".cm-livePreview-task-content": {
    textDecoration: "none",
  },
  ".cm-livePreview-hr": {
    borderTop: "1px solid #d1d5db",
    display: "block",
    height: "0",
    margin: "0.45em 0",
    width: "100%",
  },
});
