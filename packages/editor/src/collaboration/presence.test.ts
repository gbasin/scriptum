import { describe, expect, it } from "vitest";
import { parseAwarenessPeer, readAwarenessPeers } from "./presence";

function fallbackColor(name: string): string {
  return `color:${name}`;
}

describe("parseAwarenessPeer", () => {
  it("normalizes user identity and cursor payload", () => {
    expect(
      parseAwarenessPeer(
        7,
        {
          cursor: {
            anchor: 3,
            head: 5,
            line: 1,
            column: 2,
            sectionId: "sec-1",
          },
          user: {
            color: "#123456",
            name: "Alice",
            type: "agent",
          },
        },
        fallbackColor,
      ),
    ).toEqual({
      clientId: 7,
      color: "#123456",
      cursor: {
        anchor: 3,
        head: 5,
        line: 1,
        column: 2,
        sectionId: "sec-1",
      },
      name: "Alice",
      type: "agent",
    });
  });

  it("applies defaults when awareness payload is sparse", () => {
    expect(
      parseAwarenessPeer(11, { cursor: { anchor: 4 } }, fallbackColor),
    ).toEqual({
      clientId: 11,
      color: "color:User 11",
      cursor: {
        anchor: 4,
        head: 4,
      },
      name: "User 11",
      type: "human",
    });
  });

  it("returns null cursor when anchor/head are missing", () => {
    expect(
      parseAwarenessPeer(3, { user: { name: "Bob" } }, fallbackColor),
    ).toEqual({
      clientId: 3,
      color: "color:Bob",
      cursor: null,
      name: "Bob",
      type: "human",
    });
  });
});

describe("readAwarenessPeers", () => {
  it("sorts peers and can omit the local client", () => {
    const peers = readAwarenessPeers(
      new Map<number, unknown>([
        [4, { user: { name: "D" }, cursor: { anchor: 1, head: 1 } }],
        [2, { user: { name: "B" }, cursor: { anchor: 2, head: 2 } }],
        [3, { user: { name: "C" }, cursor: { anchor: 3, head: 3 } }],
      ]),
      {
        fallbackColor,
        includeLocal: false,
        localClientId: 3,
      },
    );

    expect(peers.map((peer) => peer.clientId)).toEqual([2, 4]);
  });
});
