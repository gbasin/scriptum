import { describe, expect, it, vi } from "vitest";
import { isFixtureModeEnabled, setupFixtureMode } from "./setup";

function createFakeDocument() {
  const styles: Array<{ id: string; textContent: string | null }> = [];
  const fakeHead = {
    appendChild: vi.fn(
      (element: { id: string; textContent: string | null }) => {
        styles.push(element);
      },
    ),
  };
  const fakeDocumentElement = {
    setAttribute: vi.fn(),
    style: {
      setProperty: vi.fn(),
    },
  };

  return {
    styles,
    document: {
      head: fakeHead,
      documentElement: fakeDocumentElement,
      getElementById: vi.fn(() => null),
      createElement: vi.fn(() => ({ id: "", textContent: "" })),
    } as unknown as Document,
  };
}

describe("isFixtureModeEnabled", () => {
  it("enables fixture mode in test mode or explicit env flag", () => {
    expect(isFixtureModeEnabled({ MODE: "test" })).toBe(true);
    expect(isFixtureModeEnabled({ VITE_SCRIPTUM_FIXTURE_MODE: "1" })).toBe(
      true,
    );
    expect(isFixtureModeEnabled({ VITE_SCRIPTUM_FIXTURE_MODE: "true" })).toBe(
      true,
    );
    expect(
      isFixtureModeEnabled({
        MODE: "production",
        VITE_SCRIPTUM_FIXTURE_MODE: "0",
      }),
    ).toBe(false);
  });
});

describe("setupFixtureMode", () => {
  it("freezes time and installs deterministic fixture styling", () => {
    const { styles, document } = createFakeDocument();
    const fakeWindow = {
      Date: { now: () => 42 } as DateConstructor,
      document,
    } as Window & typeof globalThis;

    const applied = setupFixtureMode({
      env: { VITE_SCRIPTUM_FIXTURE_MODE: "1" },
      globalWindow: fakeWindow,
      fixedNowMs: 1_700_000_000_000,
      viewport: { width: 1440, height: 900, deviceScaleFactor: 1 },
    });

    expect(applied).toBe(true);
    expect(fakeWindow.Date.now()).toBe(1_700_000_000_000);
    expect(styles).toHaveLength(1);
    expect(styles[0].textContent).toContain(
      "--scriptum-test-viewport-width: 1440px;",
    );
    expect(document.documentElement.setAttribute).toHaveBeenCalledWith(
      "data-scriptum-fixture-mode",
      "true",
    );
  });

  it("is idempotent and does not install duplicate styles", () => {
    const { styles, document } = createFakeDocument();
    const fakeWindow = {
      Date: { now: () => 42 } as DateConstructor,
      document,
    } as Window & typeof globalThis;

    const options = {
      env: { VITE_SCRIPTUM_FIXTURE_MODE: "1" },
      globalWindow: fakeWindow,
    };

    expect(setupFixtureMode(options)).toBe(true);
    expect(setupFixtureMode(options)).toBe(true);
    expect(styles).toHaveLength(1);
  });
});
