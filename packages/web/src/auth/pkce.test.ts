import { describe, expect, it } from "vitest";
import { generateCodeChallenge, generateCodeVerifier } from "./pkce";

describe("pkce", () => {
  it("generateCodeVerifier returns a URL-safe string", () => {
    const verifier = generateCodeVerifier();
    expect(verifier.length).toBeGreaterThan(0);
    // Base64-URL chars only (no +, /, =).
    expect(verifier).toMatch(/^[A-Za-z0-9_-]+$/);
  });

  it("generateCodeVerifier is random (two calls differ)", () => {
    const a = generateCodeVerifier();
    const b = generateCodeVerifier();
    expect(a).not.toBe(b);
  });

  it("generateCodeChallenge returns a URL-safe string", async () => {
    const verifier = generateCodeVerifier();
    const challenge = await generateCodeChallenge(verifier);
    expect(challenge.length).toBeGreaterThan(0);
    expect(challenge).toMatch(/^[A-Za-z0-9_-]+$/);
  });

  it("same verifier produces same challenge (deterministic)", async () => {
    const verifier = "fixed-test-verifier";
    const a = await generateCodeChallenge(verifier);
    const b = await generateCodeChallenge(verifier);
    expect(a).toBe(b);
  });

  it("different verifiers produce different challenges", async () => {
    const a = await generateCodeChallenge("verifier-alpha");
    const b = await generateCodeChallenge("verifier-beta");
    expect(a).not.toBe(b);
  });
});
