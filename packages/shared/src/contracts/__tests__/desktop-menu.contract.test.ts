import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const __dirname = dirname(fileURLToPath(import.meta.url));

interface MenuItem {
  id: string;
  frontend_action: string;
  shortcut: string | null;
}

interface MenuContract {
  action_event: string;
  import_dialog_event: string;
  export_dialog_event: string;
  items: MenuItem[];
}

const contract: MenuContract = JSON.parse(
  readFileSync(
    resolve(__dirname, "../../../../../contracts/desktop-menu.json"),
    "utf-8",
  ),
);

describe("desktop-menu contract", () => {
  it("contains exactly 9 menu items with expected IDs", () => {
    const expectedIds = [
      "menu.new-document",
      "menu.save-document",
      "menu.import-markdown",
      "menu.export-markdown",
      "menu.close-window",
      "menu.quit-app",
      "menu.toggle-fullscreen",
      "menu.open-help",
      "menu.open-about",
    ];
    const contractIds = contract.items.map((item) => item.id);
    expect(contractIds.sort()).toEqual(expectedIds.sort());
  });

  it("frontend actions match their menu IDs", () => {
    const expectedActions: Record<string, string> = {
      "menu.new-document": "new-document",
      "menu.save-document": "save-document",
      "menu.import-markdown": "import-markdown",
      "menu.export-markdown": "export-markdown",
      "menu.close-window": "close-window",
      "menu.quit-app": "quit-app",
      "menu.toggle-fullscreen": "toggle-fullscreen",
      "menu.open-help": "open-help",
      "menu.open-about": "open-about",
    };
    for (const item of contract.items) {
      expect(item.frontend_action).toBe(expectedActions[item.id]);
    }
  });

  it("shortcuts match expected values", () => {
    const expectedShortcuts: Record<string, string | null> = {
      "menu.new-document": "CmdOrCtrl+N",
      "menu.save-document": "CmdOrCtrl+S",
      "menu.import-markdown": null,
      "menu.export-markdown": null,
      "menu.close-window": "CmdOrCtrl+W",
      "menu.quit-app": "CmdOrCtrl+Q",
      "menu.toggle-fullscreen": "F11",
      "menu.open-help": null,
      "menu.open-about": null,
    };
    for (const item of contract.items) {
      expect(item.shortcut).toBe(expectedShortcuts[item.id]);
    }
  });

  it("event names are exact", () => {
    expect(contract.action_event).toBe("scriptum://menu-action");
    expect(contract.import_dialog_event).toBe(
      "scriptum://dialog/import-selected",
    );
    expect(contract.export_dialog_event).toBe(
      "scriptum://dialog/export-selected",
    );
  });
});
