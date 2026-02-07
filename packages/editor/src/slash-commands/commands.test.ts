import { describe, expect, it } from "vitest";
import {
  createSlashCommandInsertion,
  getSlashCommand,
  listSlashCommands,
} from "./commands.js";

describe("slash command definitions", () => {
  it("provides the canonical slash command list", () => {
    expect(listSlashCommands().map((command) => command.name)).toEqual([
      "table",
      "code",
      "image",
      "callout",
    ]);
  });

  it("builds insertion spec that replaces the slash token and places cursor", () => {
    const from = 12;
    const to = 20;
    const command = getSlashCommand("table");
    const insertion = createSlashCommandInsertion(command, from, to);

    expect(insertion.changes).toEqual({
      from,
      insert: [
        "| Column 1 | Column 2 |",
        "| --- | --- |",
        "|  |  |",
      ].join("\n"),
      to,
    });
    expect(insertion.selection.anchor).toBe(from + command.selectionOffset);
  });

  it("keeps cursor in editable part for each command template", () => {
    for (const command of listSlashCommands()) {
      const insertion = createSlashCommandInsertion(command, 0, 0);
      const offset = insertion.selection.anchor;
      expect(offset).toBeGreaterThanOrEqual(0);
      expect(offset).toBeLessThanOrEqual(insertion.changes.insert.length);
    }
  });
});
