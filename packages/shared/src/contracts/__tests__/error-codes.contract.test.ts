import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { ERROR_CODES } from "../error-codes";

const __dirname = dirname(fileURLToPath(import.meta.url));
const contract = JSON.parse(
  readFileSync(
    resolve(__dirname, "../../../../../contracts/error-codes.json"),
    "utf-8",
  ),
);

describe("error-codes contract", () => {
  it("ERROR_CODES keys match contract codes", () => {
    const contractCodes = (
      contract.codes as Array<{ code: string }>
    )
      .map((c) => c.code)
      .sort();
    expect(Object.values(ERROR_CODES).sort()).toEqual(contractCodes);
  });

  it("every contract code is a key in ERROR_CODES", () => {
    for (const entry of contract.codes as Array<{ code: string }>) {
      expect(ERROR_CODES).toHaveProperty(entry.code);
    }
  });
});
