import { describe, expect, it } from "vitest";
import { cn } from "./cn";

describe("cn", () => {
  it("merges class values with clsx semantics", () => {
    expect(
      cn("root", null, undefined, false, ["accent", { muted: true }], {
        active: false,
        selected: 1,
      }),
    ).toBe("root accent muted selected");
  });
});
