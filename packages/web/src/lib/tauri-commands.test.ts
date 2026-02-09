// Validates that tauri-commands.ts stays in sync with tauri-auth-contract.json.
// The Rust side has a mirror test in commands.rs — together they ensure both
// languages track the same contract manifest.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import {
  TAURI_AUTH_COMMANDS,
  TAURI_DEEP_LINK_EVENT,
  TAURI_REDIRECT_URI,
  type TauriAuthTokens,
  type TauriOAuthCallbackPayload,
} from "./tauri-commands";

const contractPath = resolve(
  __dirname,
  "../../../desktop/tauri-auth-contract.json",
);
const manifest = JSON.parse(readFileSync(contractPath, "utf-8"));

describe("tauri-commands contract validation", () => {
  it("TAURI_AUTH_COMMANDS covers every command in the manifest", () => {
    const manifestNames: string[] = manifest.commands.map(
      (c: { name: string }) => c.name,
    );
    const tsValues = Object.values(TAURI_AUTH_COMMANDS);

    // Every manifest command is present in TS constants
    for (const name of manifestNames) {
      expect(tsValues).toContain(name);
    }

    // No extra commands in TS that aren't in the manifest
    for (const val of tsValues) {
      expect(manifestNames).toContain(val);
    }
  });

  it("deep_link_event matches TAURI_DEEP_LINK_EVENT", () => {
    expect(TAURI_DEEP_LINK_EVENT).toBe(manifest.deep_link_event);
  });

  it("redirect_uri matches TAURI_REDIRECT_URI", () => {
    expect(TAURI_REDIRECT_URI).toBe(manifest.redirect_uri);
  });

  it("TauriAuthTokens fields match the manifest AuthTokens type", () => {
    const manifestFields: string[] = manifest.types.AuthTokens;

    // Create a dummy object with all interface fields to verify shape
    const dummy: TauriAuthTokens = {
      access_token: "",
      refresh_token: "",
      access_expires_at: null,
      refresh_expires_at: null,
    };

    const tsKeys = Object.keys(dummy).sort();
    const contractKeys = [...manifestFields].sort();

    expect(tsKeys).toEqual(contractKeys);
  });

  it("TauriOAuthCallbackPayload fields match the manifest", () => {
    const manifestFields: string[] = manifest.types.OAuthCallbackPayload;

    const dummy: TauriOAuthCallbackPayload = {
      url: "",
      code: null,
      state: null,
      error: null,
      error_description: null,
    };

    const tsKeys = Object.keys(dummy).sort();
    const contractKeys = [...manifestFields].sort();

    expect(tsKeys).toEqual(contractKeys);
  });

  it("command arg names match the manifest", () => {
    const commandMap = new Map(
      manifest.commands.map((c: { name: string; args: object }) => [
        c.name,
        c.args,
      ]),
    );

    // auth_open_browser takes authorizationUrl
    const openBrowserArgs = commandMap.get("auth_open_browser") as Record<
      string,
      string
    >;
    expect(openBrowserArgs).toHaveProperty("authorizationUrl");

    // auth_parse_callback takes url
    const parseCallbackArgs = commandMap.get("auth_parse_callback") as Record<
      string,
      string
    >;
    expect(parseCallbackArgs).toHaveProperty("url");

    // auth_store_tokens takes tokens
    const storeTokensArgs = commandMap.get("auth_store_tokens") as Record<
      string,
      string
    >;
    expect(storeTokensArgs).toHaveProperty("tokens");
  });
});
