import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const __dirname = dirname(fileURLToPath(import.meta.url));
const contract = JSON.parse(
  readFileSync(
    resolve(__dirname, "../../../../../contracts/desktop-menu.json"),
    "utf-8",
  ),
);

describe("desktop-menu contract", () => {
  it("all menu items have an id and frontend_action", () => {
    for (const item of contract.items as Array<{
      id: string;
      frontend_action: string;
    }>) {
      expect(item.id).toBeTruthy();
      expect(item.frontend_action).toBeTruthy();
    }
  });

  it("menu IDs follow the menu.* pattern", () => {
    for (const item of contract.items as Array<{ id: string }>) {
      expect(item.id).toMatch(/^menu\./);
    }
  });

  it("event names follow the scriptum:// pattern", () => {
    expect(contract.action_event).toMatch(/^scriptum:\/\//);
    expect(contract.import_dialog_event).toMatch(/^scriptum:\/\//);
    expect(contract.export_dialog_event).toMatch(/^scriptum:\/\//);
  });
});
