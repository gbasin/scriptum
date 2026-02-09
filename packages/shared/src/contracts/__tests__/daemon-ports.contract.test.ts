import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import {
  DAEMON_LOCAL_HOST,
  DAEMON_LOCAL_PORT,
  DAEMON_WHOAMI_URL,
  DAEMON_YJS_WS_URL,
} from "../daemon-ports";

const __dirname = dirname(fileURLToPath(import.meta.url));
const contract = JSON.parse(
  readFileSync(
    resolve(__dirname, "../../../../../contracts/daemon-ports.json"),
    "utf-8",
  ),
);

describe("daemon-ports contract", () => {
  it("host matches contract", () => {
    expect(DAEMON_LOCAL_HOST).toBe(contract.host);
  });

  it("port matches contract", () => {
    expect(DAEMON_LOCAL_PORT).toBe(contract.port);
  });

  it("YJS WS URL matches contract", () => {
    expect(DAEMON_YJS_WS_URL).toBe(contract.yjs_ws_url);
  });

  it("whoami URL matches contract", () => {
    expect(DAEMON_WHOAMI_URL).toBe(contract.whoami_url);
  });
});
