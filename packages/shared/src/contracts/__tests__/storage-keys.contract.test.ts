import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { STORAGE_KEYS } from "../storage-keys";

const __dirname = dirname(fileURLToPath(import.meta.url));
const contract = JSON.parse(
  readFileSync(
    resolve(__dirname, "../../../../../contracts/storage-keys.json"),
    "utf-8",
  ),
);

describe("storage-keys contract", () => {
  it("STORAGE_KEYS match contract keys", () => {
    expect({ ...STORAGE_KEYS }).toEqual(contract.keys);
  });

  it("all keys have the contract prefix", () => {
    for (const value of Object.values(STORAGE_KEYS)) {
      expect(value).toMatch(new RegExp(`^${contract.prefix}`));
    }
  });
});
