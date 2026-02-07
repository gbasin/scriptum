export type SlashCommandName = "table" | "code" | "image" | "callout";

export interface SlashCommandDefinition {
  readonly detail: string;
  readonly insert: string;
  readonly name: SlashCommandName;
  readonly selectionOffset: number;
}

export interface SlashCommandInsertion {
  readonly changes: {
    readonly from: number;
    readonly insert: string;
    readonly to: number;
  };
  readonly selection: {
    readonly anchor: number;
  };
}

function createTemplate(rawTemplate: string): {
  readonly insert: string;
  readonly selectionOffset: number;
} {
  const cursorToken = "<$cursor$>";
  const selectionOffset = rawTemplate.indexOf(cursorToken);
  if (selectionOffset === -1) {
    throw new Error("Slash command template is missing the cursor token.");
  }

  return {
    insert: rawTemplate.replace(cursorToken, ""),
    selectionOffset,
  };
}

const tableTemplate = createTemplate(
  ["| Column 1 | Column 2 |", "| --- | --- |", "| <$cursor$> |  |"].join("\n"),
);

const codeTemplate = createTemplate(["```ts", "<$cursor$>", "```"].join("\n"));

const imageTemplate = createTemplate(
  "![<$cursor$>alt text](https://example.com/image.png)",
);

const calloutTemplate = createTemplate(
  ["> [!NOTE]", "> <$cursor$>"].join("\n"),
);

const slashCommandDefinitionsInternal: readonly SlashCommandDefinition[] = [
  {
    detail: "Insert a 2-column markdown table.",
    ...tableTemplate,
    name: "table",
  },
  {
    detail: "Insert a fenced TypeScript code block.",
    ...codeTemplate,
    name: "code",
  },
  {
    detail: "Insert markdown image syntax.",
    ...imageTemplate,
    name: "image",
  },
  {
    detail: "Insert an Obsidian-style markdown callout block.",
    ...calloutTemplate,
    name: "callout",
  },
];

const slashCommandByName = new Map(
  slashCommandDefinitionsInternal.map(
    (command) => [command.name, command] as const,
  ),
);

export function listSlashCommands(): readonly SlashCommandDefinition[] {
  return slashCommandDefinitionsInternal;
}

export function getSlashCommand(
  name: SlashCommandName,
): SlashCommandDefinition {
  const command = slashCommandByName.get(name);
  if (!command) {
    throw new Error(`Unknown slash command: ${name}`);
  }
  return command;
}

export function createSlashCommandInsertion(
  command: SlashCommandDefinition,
  from: number,
  to: number,
): SlashCommandInsertion {
  return {
    changes: {
      from,
      insert: command.insert,
      to,
    },
    selection: {
      anchor: from + command.selectionOffset,
    },
  };
}
