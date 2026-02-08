// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToString } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ShareDialog } from "./ShareDialog";

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

afterEach(() => {
  document.body.innerHTML = "";
  globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
});

function baseProps() {
  return {
    documentId: "doc-1",
    generatedShareUrl: "",
    onClose: vi.fn(),
    onExpirationOptionChange: vi.fn(),
    onGenerate: vi.fn(),
    onMaxUsesInputChange: vi.fn(),
    onPermissionChange: vi.fn(),
    onTargetTypeChange: vi.fn(),
    shareExpirationOption: "none" as const,
    shareMaxUsesInput: "3",
    sharePermission: "view" as const,
    shareTargetType: "document" as const,
    summaryPermissionLabel: "Viewer",
  };
}

describe("ShareDialog", () => {
  it("renders a user-visible generation error message", () => {
    const html = renderToString(
      <ShareDialog
        {...baseProps()}
        generationError="Network request failed. Please try again."
      />,
    );

    expect(html).toContain('data-testid="share-link-error"');
    expect(html).toContain("Network request failed. Please try again.");
  });

  it("invokes onGenerate when the generate button is clicked", () => {
    globalThis.IS_REACT_ACT_ENVIRONMENT = true;
    const onGenerate = vi.fn();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    act(() => {
      root.render(<ShareDialog {...baseProps()} onGenerate={onGenerate} />);
    });

    const button = container.querySelector(
      '[data-testid="share-link-generate"]',
    ) as HTMLButtonElement | null;
    expect(button).not.toBeNull();

    act(() => {
      button?.click();
    });

    expect(onGenerate).toHaveBeenCalledTimes(1);

    act(() => {
      root.unmount();
    });
  });
});
