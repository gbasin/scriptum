import {
  type EditorState,
  type Extension,
  Facet,
  StateEffect,
  StateField,
} from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  WidgetType,
} from "@codemirror/view";

export const RECONCILIATION_KEEP_BOTH_SEPARATOR = "\n\n---\n\n";

export type ReconciliationChoice = "keep-a" | "keep-b" | "keep-both";

export interface ReconciliationInlineVersion {
  readonly authorId: string;
  readonly authorName?: string;
  readonly content: string;
}

export interface ReconciliationInlineEntry {
  readonly id: string;
  readonly sectionId: string;
  readonly from: number;
  readonly to: number;
  readonly versionA: ReconciliationInlineVersion;
  readonly versionB: ReconciliationInlineVersion;
  readonly triggeredAtMs?: number;
}

export interface ReconciliationInlineResolution {
  readonly id: string;
  readonly sectionId: string;
  readonly choice: ReconciliationChoice;
  readonly replacement: string;
  readonly from: number;
  readonly to: number;
  readonly triggeredAtMs?: number;
}

export interface ReconciliationInlineExtensionOptions {
  readonly keepBothSeparator?: string;
  readonly onResolve?: (resolution: ReconciliationInlineResolution) => void;
}

interface NormalizedReconciliationInlineVersion {
  readonly authorId: string;
  readonly authorName: string;
  readonly content: string;
}

interface NormalizedReconciliationInlineEntry {
  readonly id: string;
  readonly sectionId: string;
  readonly from: number;
  readonly to: number;
  readonly versionA: NormalizedReconciliationInlineVersion;
  readonly versionB: NormalizedReconciliationInlineVersion;
  readonly triggeredAtMs?: number;
}

interface ReconciliationInlineStateValue {
  readonly entries: readonly NormalizedReconciliationInlineEntry[];
  readonly decorations: DecorationSet;
}

interface ReconciliationInlineRuntimeConfig {
  readonly keepBothSeparator: string;
  readonly onResolve?: (resolution: ReconciliationInlineResolution) => void;
}

const DEFAULT_RUNTIME_CONFIG: ReconciliationInlineRuntimeConfig = {
  keepBothSeparator: RECONCILIATION_KEEP_BOTH_SEPARATOR,
};

export const setReconciliationInlineEntries =
  StateEffect.define<readonly ReconciliationInlineEntry[]>();

const removeReconciliationInlineEntry = StateEffect.define<string>();

const reconciliationRuntimeConfig = Facet.define<
  ReconciliationInlineRuntimeConfig,
  ReconciliationInlineRuntimeConfig
>({
  combine(values) {
    if (values.length === 0) {
      return DEFAULT_RUNTIME_CONFIG;
    }
    return values[0];
  },
});

export const reconciliationInlineState =
  StateField.define<ReconciliationInlineStateValue>({
    create(state) {
      const entries: readonly NormalizedReconciliationInlineEntry[] = [];
      return {
        entries,
        decorations: buildDecorations(state, entries),
      };
    },
    provide: (field) =>
      EditorView.decorations.from(field, (value) => value.decorations),
    update(current, transaction) {
      let nextEntries = current.entries;

      for (const effect of transaction.effects) {
        if (effect.is(setReconciliationInlineEntries)) {
          nextEntries = normalizeEntries(transaction.state, effect.value);
        } else if (effect.is(removeReconciliationInlineEntry)) {
          nextEntries = nextEntries.filter(
            (entry) => entry.id !== effect.value,
          );
        }
      }

      if (nextEntries === current.entries && transaction.docChanged) {
        nextEntries = mapEntriesThroughChanges(
          transaction.state,
          current.entries,
          transaction.changes,
        );
      }

      if (nextEntries === current.entries) {
        return current;
      }

      return {
        entries: nextEntries,
        decorations: buildDecorations(transaction.state, nextEntries),
      };
    },
  });

const reconciliationInlineTheme = EditorView.baseTheme({
  ".cm-reconciliationInline": {
    backgroundColor: "rgba(14, 116, 144, 0.06)",
    border: "1px solid rgba(14, 116, 144, 0.24)",
    borderRadius: "0.5rem",
    boxSizing: "border-box",
    display: "flex",
    flexDirection: "column",
    gap: "0.625rem",
    margin: "0.75rem 0",
    padding: "0.75rem",
  },
  ".cm-reconciliationInline-version": {
    backgroundColor: "rgba(255, 255, 255, 0.8)",
    border: "1px solid rgba(148, 163, 184, 0.4)",
    borderRadius: "0.4rem",
    padding: "0.5rem 0.625rem",
  },
  ".cm-reconciliationInline-versionHeader": {
    color: "#0f172a",
    fontSize: "0.8rem",
    fontWeight: "600",
    marginBottom: "0.375rem",
  },
  ".cm-reconciliationInline-versionBody": {
    color: "#1f2937",
    fontFamily:
      'ui-monospace, "SFMono-Regular", Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
    fontSize: "0.8rem",
    lineHeight: "1.45",
    margin: 0,
    whiteSpace: "pre-wrap",
  },
  ".cm-reconciliationInline-divider": {
    backgroundColor: "rgba(148, 163, 184, 0.5)",
    height: "1px",
    width: "100%",
  },
  ".cm-reconciliationInline-actions": {
    display: "flex",
    flexWrap: "wrap",
    gap: "0.5rem",
  },
  ".cm-reconciliationInline-button": {
    backgroundColor: "#f8fafc",
    border: "1px solid rgba(100, 116, 139, 0.5)",
    borderRadius: "0.375rem",
    color: "#0f172a",
    cursor: "pointer",
    fontSize: "0.8rem",
    fontWeight: "600",
    lineHeight: "1.2",
    padding: "0.28rem 0.6rem",
  },
  ".cm-reconciliationInline-button:hover": {
    backgroundColor: "#e2e8f0",
  },
});

class ReconciliationWidget extends WidgetType {
  constructor(private readonly entry: NormalizedReconciliationInlineEntry) {
    super();
  }

  override eq(other: WidgetType): boolean {
    return (
      other instanceof ReconciliationWidget &&
      areEntriesEqual([this.entry], [other.entry])
    );
  }

  override toDOM(view: EditorView): HTMLElement {
    const root = document.createElement("section");
    root.className = "cm-reconciliationInline";
    root.dataset.reconciliationId = this.entry.id;

    root.appendChild(buildVersionBlock("A", this.entry.versionA));
    root.appendChild(createDivider());
    root.appendChild(buildVersionBlock("B", this.entry.versionB));
    root.appendChild(buildActionButtons(view, this.entry.id));

    return root;
  }
}

export function reconciliationInlineExtension(
  options: ReconciliationInlineExtensionOptions = {},
): Extension {
  return [
    reconciliationRuntimeConfig.of({
      keepBothSeparator: normalizeKeepBothSeparator(options.keepBothSeparator),
      onResolve: options.onResolve,
    }),
    reconciliationInlineState,
    reconciliationInlineTheme,
  ];
}

function normalizeEntries(
  state: EditorState,
  entries: readonly ReconciliationInlineEntry[],
): readonly NormalizedReconciliationInlineEntry[] {
  if (entries.length === 0) {
    return [];
  }

  const maxPosition = state.doc.length;
  const byId = new Map<string, NormalizedReconciliationInlineEntry>();

  for (const entry of entries) {
    const id = normalizeNonEmpty(entry.id);
    if (!id) {
      continue;
    }

    const sectionId = normalizeNonEmpty(entry.sectionId) ?? id;
    const versionA = normalizeVersion(entry.versionA);
    const versionB = normalizeVersion(entry.versionB);
    if (!versionA || !versionB) {
      continue;
    }

    const from = clampPosition(entry.from, maxPosition);
    const to = clampPosition(entry.to, maxPosition);

    byId.set(id, {
      id,
      sectionId,
      from: Math.min(from, to),
      to: Math.max(from, to),
      versionA,
      versionB,
      triggeredAtMs: entry.triggeredAtMs,
    });
  }

  return sortEntries(Array.from(byId.values()));
}

function normalizeVersion(
  version: ReconciliationInlineVersion,
): NormalizedReconciliationInlineVersion | null {
  const authorId = normalizeNonEmpty(version.authorId);
  if (!authorId) {
    return null;
  }

  const authorName = normalizeNonEmpty(version.authorName) ?? authorId;
  return {
    authorId,
    authorName,
    content: String(version.content),
  };
}

function normalizeNonEmpty(value: string | undefined): string | null {
  if (typeof value !== "string") {
    return null;
  }
  const normalized = value.trim();
  return normalized.length > 0 ? normalized : null;
}

function clampPosition(position: number, maxPosition: number): number {
  if (!Number.isFinite(position)) {
    return 0;
  }
  const rounded = Math.floor(position);
  if (rounded < 0) {
    return 0;
  }
  if (rounded > maxPosition) {
    return maxPosition;
  }
  return rounded;
}

function sortEntries(
  entries: readonly NormalizedReconciliationInlineEntry[],
): readonly NormalizedReconciliationInlineEntry[] {
  return [...entries].sort((left, right) => {
    if (left.from !== right.from) {
      return left.from - right.from;
    }
    return left.id.localeCompare(right.id);
  });
}

function buildDecorations(
  _state: EditorState,
  entries: readonly NormalizedReconciliationInlineEntry[],
): DecorationSet {
  if (entries.length === 0) {
    return Decoration.none;
  }

  const decorations = entries.map((entry) =>
    Decoration.widget({
      block: true,
      side: 1,
      widget: new ReconciliationWidget(entry),
    }).range(entry.to),
  );

  return Decoration.set(decorations, true);
}

function buildVersionBlock(
  slot: "A" | "B",
  version: NormalizedReconciliationInlineVersion,
): HTMLElement {
  const versionRoot = document.createElement("div");
  versionRoot.className = "cm-reconciliationInline-version";
  versionRoot.dataset.versionSlot = slot;

  const header = document.createElement("div");
  header.className = "cm-reconciliationInline-versionHeader";
  header.textContent = `Version ${slot} by ${version.authorName}`;

  const body = document.createElement("pre");
  body.className = "cm-reconciliationInline-versionBody";
  body.textContent = version.content;

  versionRoot.appendChild(header);
  versionRoot.appendChild(body);
  return versionRoot;
}

function createDivider(): HTMLElement {
  const divider = document.createElement("div");
  divider.className = "cm-reconciliationInline-divider";
  divider.setAttribute("aria-hidden", "true");
  return divider;
}

function buildActionButtons(view: EditorView, entryId: string): HTMLElement {
  const actions = document.createElement("div");
  actions.className = "cm-reconciliationInline-actions";

  const choices: readonly { choice: ReconciliationChoice; label: string }[] = [
    { choice: "keep-a", label: "Keep A" },
    { choice: "keep-b", label: "Keep B" },
    { choice: "keep-both", label: "Keep Both" },
  ];

  for (const { choice, label } of choices) {
    const button = document.createElement("button");
    button.className = "cm-reconciliationInline-button";
    button.dataset.choice = choice;
    button.textContent = label;
    button.type = "button";
    button.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      resolveEntryChoice(view, entryId, choice);
    });
    actions.appendChild(button);
  }

  return actions;
}

function resolveEntryChoice(
  view: EditorView,
  entryId: string,
  choice: ReconciliationChoice,
): void {
  const state = view.state;
  const entryState = state.field(reconciliationInlineState, false);
  if (!entryState) {
    return;
  }

  const entry = entryState.entries.find(
    (candidate) => candidate.id === entryId,
  );
  if (!entry) {
    return;
  }

  const config = state.facet(reconciliationRuntimeConfig);
  const replacement = buildReplacement(entry, choice, config.keepBothSeparator);

  view.dispatch({
    changes: { from: entry.from, to: entry.to, insert: replacement },
    effects: [removeReconciliationInlineEntry.of(entry.id)],
  });

  config.onResolve?.({
    id: entry.id,
    sectionId: entry.sectionId,
    choice,
    replacement,
    from: entry.from,
    to: entry.to,
    triggeredAtMs: entry.triggeredAtMs,
  });
}

function buildReplacement(
  entry: NormalizedReconciliationInlineEntry,
  choice: ReconciliationChoice,
  keepBothSeparator: string,
): string {
  if (choice === "keep-a") {
    return entry.versionA.content;
  }
  if (choice === "keep-b") {
    return entry.versionB.content;
  }

  const segments = [entry.versionA.content, entry.versionB.content].filter(
    (content) => content.length > 0,
  );
  return segments.join(keepBothSeparator);
}

function normalizeKeepBothSeparator(separator: string | undefined): string {
  if (separator === undefined) {
    return RECONCILIATION_KEEP_BOTH_SEPARATOR;
  }
  return separator.length > 0 ? separator : RECONCILIATION_KEEP_BOTH_SEPARATOR;
}

function mapEntriesThroughChanges(
  state: EditorState,
  entries: readonly NormalizedReconciliationInlineEntry[],
  changes: { mapPos: (pos: number, assoc?: number) => number },
): readonly NormalizedReconciliationInlineEntry[] {
  if (entries.length === 0) {
    return entries;
  }

  const maxPosition = state.doc.length;
  let changed = false;
  const mappedEntries = entries.map((entry) => {
    const mappedFrom = clampPosition(
      changes.mapPos(entry.from, 1),
      maxPosition,
    );
    const mappedTo = clampPosition(changes.mapPos(entry.to, -1), maxPosition);
    const nextFrom = Math.min(mappedFrom, mappedTo);
    const nextTo = Math.max(mappedFrom, mappedTo);
    if (nextFrom !== entry.from || nextTo !== entry.to) {
      changed = true;
    }
    return {
      ...entry,
      from: nextFrom,
      to: nextTo,
    };
  });

  return changed ? sortEntries(mappedEntries) : entries;
}

function areEntriesEqual(
  left: readonly NormalizedReconciliationInlineEntry[],
  right: readonly NormalizedReconciliationInlineEntry[],
): boolean {
  if (left.length !== right.length) {
    return false;
  }

  for (let index = 0; index < left.length; index += 1) {
    const leftEntry = left[index];
    const rightEntry = right[index];
    if (
      leftEntry.id !== rightEntry.id ||
      leftEntry.sectionId !== rightEntry.sectionId ||
      leftEntry.from !== rightEntry.from ||
      leftEntry.to !== rightEntry.to ||
      leftEntry.triggeredAtMs !== rightEntry.triggeredAtMs ||
      leftEntry.versionA.authorId !== rightEntry.versionA.authorId ||
      leftEntry.versionA.authorName !== rightEntry.versionA.authorName ||
      leftEntry.versionA.content !== rightEntry.versionA.content ||
      leftEntry.versionB.authorId !== rightEntry.versionB.authorId ||
      leftEntry.versionB.authorName !== rightEntry.versionB.authorName ||
      leftEntry.versionB.content !== rightEntry.versionB.content
    ) {
      return false;
    }
  }

  return true;
}
