import { markdownLanguage } from "@codemirror/lang-markdown";
import { type EditorState, StateField } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView } from "@codemirror/view";
import type { SyntaxNode } from "@lezer/common";
import { rangeTouchesActiveLine } from "./shared";

const EMPHASIS_CLASS_BY_NODE: Readonly<Record<string, string>> = {
  Emphasis: "cm-livePreview-emphasis",
  StrongEmphasis: "cm-livePreview-strong",
  Strikethrough: "cm-livePreview-strikethrough",
};

function isInlineMarkNode(name: string): boolean {
  return name === "EmphasisMark" || name === "StrikethroughMark";
}

function addInlineEmphasisDecorations(
  decorations: Array<{ from: number; to: number; decoration: Decoration }>,
  state: EditorState,
  node: SyntaxNode,
  className: string,
): void {
  if (rangeTouchesActiveLine(state, node.from, node.to)) {
    return;
  }

  let contentFrom = node.from;
  let contentTo = node.to;

  const firstChild = node.firstChild;
  if (
    firstChild &&
    isInlineMarkNode(firstChild.type.name) &&
    firstChild.from === node.from
  ) {
    decorations.push({
      from: firstChild.from,
      to: firstChild.to,
      decoration: Decoration.replace({
        inclusive: false,
      }),
    });
    contentFrom = firstChild.to;
  }

  const lastChild = node.lastChild;
  if (
    lastChild &&
    isInlineMarkNode(lastChild.type.name) &&
    lastChild.to === node.to
  ) {
    decorations.push({
      from: lastChild.from,
      to: lastChild.to,
      decoration: Decoration.replace({
        inclusive: false,
      }),
    });
    contentTo = lastChild.from;
  }

  if (contentFrom < contentTo) {
    decorations.push({
      from: contentFrom,
      to: contentTo,
      decoration: Decoration.mark({
        class: className,
      }),
    });
  }
}

function buildInlineEmphasisDecorations(state: EditorState): DecorationSet {
  const decorations: Array<{
    from: number;
    to: number;
    decoration: Decoration;
  }> = [];
  const tree = markdownLanguage.parser.parse(state.doc.toString());

  function walk(node: SyntaxNode): void {
    const className = EMPHASIS_CLASS_BY_NODE[node.type.name];
    if (className) {
      addInlineEmphasisDecorations(decorations, state, node, className);
    }

    let child = node.firstChild;
    while (child) {
      walk(child);
      child = child.nextSibling;
    }
  }

  walk(tree.topNode);

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

export const inlineEmphasisDecorations = StateField.define<DecorationSet>({
  create: buildInlineEmphasisDecorations,
  update(currentDecorations, transaction) {
    if (!transaction.docChanged && !transaction.selection) {
      return currentDecorations;
    }

    return buildInlineEmphasisDecorations(transaction.state);
  },
  provide: (field) => EditorView.decorations.from(field),
});

export const inlineEmphasisTheme = EditorView.baseTheme({
  ".cm-livePreview-emphasis": {
    fontStyle: "italic",
  },
  ".cm-livePreview-strong": {
    fontWeight: "700",
  },
  ".cm-livePreview-strikethrough": {
    textDecoration: "line-through",
    textDecorationThickness: "from-font",
  },
});
