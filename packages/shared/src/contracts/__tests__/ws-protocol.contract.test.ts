import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type { WsMessageType } from "../../protocol/ws";
import {
  CURRENT_WS_PROTOCOL_VERSION,
  SUPPORTED_WS_PROTOCOL_VERSIONS,
} from "../../protocol/ws";

const __dirname = dirname(fileURLToPath(import.meta.url));
const contract = JSON.parse(
  readFileSync(
    resolve(__dirname, "../../../../../contracts/ws-protocol.json"),
    "utf-8",
  ),
);

// Compile-time proof that these string literals are assignable to WsMessageType.
const wsMessageTypes: Record<WsMessageType, true> = {
  hello: true,
  hello_ack: true,
  subscribe: true,
  yjs_update: true,
  ack: true,
  snapshot: true,
  awareness_update: true,
  error: true,
};

describe("ws-protocol contract", () => {
  it("WsMessageType values match contract message_types", () => {
    expect(Object.keys(wsMessageTypes).sort()).toEqual(
      [...(contract.message_types as string[])].sort(),
    );
  });

  it("current protocol version matches contract", () => {
    expect(CURRENT_WS_PROTOCOL_VERSION).toBe(contract.current_version);
  });

  it("supported protocol versions match contract", () => {
    expect([...SUPPORTED_WS_PROTOCOL_VERSIONS]).toEqual(
      contract.protocol_versions,
    );
  });
});
