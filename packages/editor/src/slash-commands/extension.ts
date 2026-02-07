import {
  autocompletion,
  type Completion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete";
import type { Extension } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import {
  createSlashCommandInsertion,
  getSlashCommand,
  listSlashCommands,
  type SlashCommandDefinition,
  type SlashCommandName,
} from "./commands";

interface SlashCommandMatch {
  readonly from: number;
  readonly query: string;
}

function matchSlashCommand(context: CompletionContext): SlashCommandMatch | null {
  const line = context.state.doc.lineAt(context.pos);
  const textBeforeCursor = line.text.slice(0, context.pos - line.from);
  const slashIndex = textBeforeCursor.lastIndexOf("/");
  if (slashIndex === -1) {
    return null;
  }

  const prefix = textBeforeCursor.slice(0, slashIndex);
  if (prefix.length > 0 && !/\s$/.test(prefix)) {
    return null;
  }

  const query = textBeforeCursor.slice(slashIndex + 1);
  if (!/^[a-z]*$/i.test(query)) {
    return null;
  }

  return {
    from: line.from + slashIndex,
    query: query.toLowerCase(),
  };
}

function slashCompletionFor(command: SlashCommandDefinition): Completion {
  return {
    apply(view, _completion, from, to) {
      applySlashCommand(view, command.name, from, to);
    },
    detail: command.detail,
    label: `/${command.name}`,
    type: "keyword",
  };
}

export function slashCommandCompletions(
  context: CompletionContext,
): CompletionResult | null {
  const match = matchSlashCommand(context);
  if (!match) {
    return null;
  }

  const options = listSlashCommands()
    .filter((command) => command.name.startsWith(match.query))
    .map(slashCompletionFor);
  if (options.length === 0) {
    return null;
  }

  return {
    filter: false,
    from: match.from,
    options,
    to: context.pos,
  };
}

export function applySlashCommand(
  view: Pick<EditorView, "dispatch">,
  commandName: SlashCommandName,
  from: number,
  to: number,
): void {
  const command = getSlashCommand(commandName);
  view.dispatch(createSlashCommandInsertion(command, from, to));
}

export function slashCommands(): Extension {
  return autocompletion({
    activateOnTyping: true,
    override: [slashCommandCompletions],
  });
}

export const slashCommandsExtension = slashCommands;
