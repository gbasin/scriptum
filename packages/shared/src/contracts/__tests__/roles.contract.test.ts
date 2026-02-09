import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { DEFAULT_ASSIGNABLE_ROLES, WORKSPACE_ROLES } from "../roles";

const __dirname = dirname(fileURLToPath(import.meta.url));
const contract = JSON.parse(
  readFileSync(
    resolve(__dirname, "../../../../../contracts/roles.json"),
    "utf-8",
  ),
);

describe("roles contract", () => {
  it("WORKSPACE_ROLES match contract roles", () => {
    expect([...WORKSPACE_ROLES]).toEqual(contract.roles);
  });

  it("DEFAULT_ASSIGNABLE_ROLES match contract default_assignable_roles", () => {
    expect([...DEFAULT_ASSIGNABLE_ROLES]).toEqual(
      contract.default_assignable_roles,
    );
  });
});
