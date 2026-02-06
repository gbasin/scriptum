import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export type SyncState = "synced" | "offline" | "reconnecting" | "error";

export interface CursorPosition {
  line: number;
  ch: number;
}

export interface RemotePeer {
  name: string;
  type: "human" | "agent";
  cursor: CursorPosition;
  section?: string;
}

export interface GitStatus {
  dirty: boolean;
  ahead: number;
  behind: number;
  lastCommit?: string;
}

export interface SmokeFixtureState {
  fixtureName?: string;
  docContent?: string;
  cursor?: CursorPosition;
  remotePeers?: RemotePeer[];
  syncState?: SyncState;
  gitStatus?: GitStatus;
}

export interface SmokeFixtureExpectations {
  heading: string;
  syncState?: SyncState;
  remotePeerCount?: number;
}

export interface SmokeFixture {
  name: string;
  route: string;
  state?: SmokeFixtureState;
  expectations: SmokeFixtureExpectations;
}

const currentFile = fileURLToPath(import.meta.url);
const fixtureDir = path.resolve(path.dirname(currentFile), "../fixtures");

export function loadSmokeFixtures(): SmokeFixture[] {
  const fixtureFiles = readdirSync(fixtureDir)
    .filter((entry) => entry.endsWith(".json"))
    .sort();

  const fixtures = fixtureFiles.map((fileName) => {
    const raw = readFileSync(path.join(fixtureDir, fileName), "utf8");
    return JSON.parse(raw) as SmokeFixture;
  });

  validateFixtureSet(fixtures);
  return fixtures;
}

function validateFixtureSet(fixtures: SmokeFixture[]): void {
  if (fixtures.length < 5 || fixtures.length > 10) {
    throw new Error(`expected 5-10 smoke fixtures, found ${fixtures.length}`);
  }

  const seenNames = new Set<string>();
  for (const fixture of fixtures) {
    if (!fixture.name || !fixture.route) {
      throw new Error("fixture requires `name` and `route`");
    }
    if (!fixture.expectations?.heading) {
      throw new Error(`fixture ${fixture.name} missing expectations.heading`);
    }
    if (seenNames.has(fixture.name)) {
      throw new Error(`duplicate fixture name: ${fixture.name}`);
    }
    seenNames.add(fixture.name);
  }
}
