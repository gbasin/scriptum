export interface FixtureViewport {
  width: number;
  height: number;
  deviceScaleFactor: number;
}

export interface SetupFixtureModeOptions {
  env?: Record<string, unknown>;
  globalWindow?: Window & typeof globalThis;
  fixedNowMs?: number;
  viewport?: FixtureViewport;
}

const FIXTURE_STYLE_ID = "scriptum-fixture-mode-style";
const DEFAULT_FIXED_NOW_MS = Date.UTC(2025, 0, 1, 0, 0, 0);
const DEFAULT_VIEWPORT: FixtureViewport = {
  width: 1280,
  height: 800,
  deviceScaleFactor: 1,
};

interface FixtureKatexRenderOptions {
  displayMode?: boolean;
}

interface FixtureKatexRenderer {
  render(
    expression: string,
    element: HTMLElement,
    options?: FixtureKatexRenderOptions,
  ): void;
}

interface FixtureMermaidRenderer {
  render(id: string, source: string): { svg: string };
}

declare global {
  interface Window {
    __SCRIPTUM_FIXTURE_SETUP_DONE__?: boolean;
    __SCRIPTUM_ORIGINAL_DATE_NOW__?: () => number;
    __SCRIPTUM_FIXED_NOW__?: number;
  }
}

function fixtureCss(viewport: FixtureViewport): string {
  return `
:root[data-scriptum-fixture-mode="true"] {
  --scriptum-test-viewport-width: ${viewport.width}px;
  --scriptum-test-viewport-height: ${viewport.height}px;
  --scriptum-test-device-scale-factor: ${viewport.deviceScaleFactor};
  --scriptum-fixture-font-family: "Scriptum Test Sans", "Helvetica Neue", Arial, sans-serif;
}
:root[data-scriptum-fixture-mode="true"],
:root[data-scriptum-fixture-mode="true"] body {
  font-family: var(--scriptum-fixture-font-family) !important;
  width: var(--scriptum-test-viewport-width);
  min-height: var(--scriptum-test-viewport-height);
}
:root[data-scriptum-fixture-mode="true"] *,
:root[data-scriptum-fixture-mode="true"] *::before,
:root[data-scriptum-fixture-mode="true"] *::after {
  animation: none !important;
  transition: none !important;
}
:root[data-scriptum-fixture-mode="true"] .cm-cursor,
:root[data-scriptum-fixture-mode="true"] .cm-dropCursor {
  animation: none !important;
}
`.trim();
}

function installFixtureStyle(
  targetDocument: Document,
  viewport: FixtureViewport,
): void {
  const existing = targetDocument.getElementById(FIXTURE_STYLE_ID);
  if (existing) {
    return;
  }

  const style = targetDocument.createElement("style");
  style.id = FIXTURE_STYLE_ID;
  style.textContent = fixtureCss(viewport);
  (targetDocument.head ?? targetDocument.documentElement).appendChild(style);
}

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function installFixtureRenderers(
  targetWindow: Window & typeof globalThis,
): void {
  const record = targetWindow as unknown as {
    katex?: FixtureKatexRenderer;
    mermaid?: FixtureMermaidRenderer;
  };

  if (!record.katex) {
    record.katex = {
      render(expression, element, options) {
        const doc = element.ownerDocument;
        element.textContent = "";

        const katexNode = doc.createElement("span");
        katexNode.className = "katex";
        katexNode.textContent = expression;

        if (options?.displayMode) {
          const displayNode = doc.createElement("span");
          displayNode.className = "katex-display";
          displayNode.appendChild(katexNode);
          element.appendChild(displayNode);
          return;
        }

        element.appendChild(katexNode);
      },
    };
  }

  if (!record.mermaid) {
    record.mermaid = {
      render(id, source) {
        return {
          svg: `<svg data-testid="fixture-mermaid" data-id="${escapeHtml(id)}" viewBox="0 0 320 60" xmlns="http://www.w3.org/2000/svg"><text x="8" y="32">${escapeHtml(source)}</text></svg>`,
        };
      },
    };
  }
}

export function isFixtureModeEnabled(
  env: Record<string, unknown> = import.meta.env as Record<string, unknown>,
): boolean {
  const explicitFlag = env.VITE_SCRIPTUM_FIXTURE_MODE;
  return env.MODE === "test" || explicitFlag === "1" || explicitFlag === "true";
}

export function setupFixtureMode(
  options: SetupFixtureModeOptions = {},
): boolean {
  if (!isFixtureModeEnabled(options.env)) {
    return false;
  }

  const targetWindow =
    options.globalWindow ??
    (typeof window === "undefined" ? undefined : window);
  if (!targetWindow) {
    return false;
  }

  if (targetWindow.__SCRIPTUM_FIXTURE_SETUP_DONE__) {
    return true;
  }

  const fixedNowMs = options.fixedNowMs ?? DEFAULT_FIXED_NOW_MS;
  const viewport = options.viewport ?? DEFAULT_VIEWPORT;

  targetWindow.__SCRIPTUM_ORIGINAL_DATE_NOW__ ??= targetWindow.Date.now.bind(
    targetWindow.Date,
  );
  targetWindow.Date.now = () => fixedNowMs;
  targetWindow.__SCRIPTUM_FIXED_NOW__ = fixedNowMs;

  if (targetWindow.document) {
    installFixtureStyle(targetWindow.document, viewport);
    installFixtureRenderers(targetWindow);
    targetWindow.document.documentElement.setAttribute(
      "data-scriptum-fixture-mode",
      "true",
    );
  }

  targetWindow.__SCRIPTUM_FIXTURE_SETUP_DONE__ = true;
  return true;
}
