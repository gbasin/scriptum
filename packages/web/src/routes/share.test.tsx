// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiClientError } from "../lib/api-client";
import { ShareRedeemRoute } from "./share";

const mockNavigate = vi.fn();
const mockRedeemShareLink = vi.fn();

vi.mock("react-router-dom", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-router-dom")>();
  return {
    ...actual,
    useNavigate: () => mockNavigate,
  };
});

vi.mock("../lib/api-client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/api-client")>();
  return {
    ...actual,
    redeemShareLink: (...args: unknown[]) => mockRedeemShareLink(...args),
  };
});

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}

function renderRoute(path: string): { container: HTMLDivElement; root: Root } {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);

  act(() => {
    root.render(
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/share/:shareToken" element={<ShareRedeemRoute />} />
        </Routes>
      </MemoryRouter>,
    );
  });

  return { container, root };
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  mockNavigate.mockReset();
  mockRedeemShareLink.mockReset();
});

afterEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = undefined;
  document.body.innerHTML = "";
});

describe("ShareRedeemRoute", () => {
  it("redeems token and redirects to the document route", async () => {
    mockRedeemShareLink.mockResolvedValue({
      workspace_id: "ws-1",
      target_type: "document",
      target_id: "doc-1",
      permission: "view",
      remaining_uses: 2,
    });

    const { root } = renderRoute("/share/token-1");

    await act(async () => {
      await Promise.resolve();
    });

    expect(mockRedeemShareLink).toHaveBeenCalledWith({ token: "token-1" });
    expect(mockNavigate).toHaveBeenCalledWith(
      "/workspace/ws-1/document/doc-1",
      { replace: true },
    );

    act(() => {
      root.unmount();
    });
  });

  it("prompts for password when redeem endpoint requires one", async () => {
    mockRedeemShareLink
      .mockRejectedValueOnce(
        new ApiClientError(
          400,
          "POST",
          "https://relay.scriptum.dev/v1/share-links/redeem",
          "SHARE_LINK_PASSWORD_REQUIRED",
          "password required",
          false,
          null,
          null,
        ),
      )
      .mockResolvedValueOnce({
        workspace_id: "ws-2",
        target_type: "workspace",
        target_id: "ws-2",
        permission: "view",
        remaining_uses: null,
      });

    const { container, root } = renderRoute("/share/token-2");

    await act(async () => {
      await Promise.resolve();
    });

    const passwordField = container.querySelector(
      '[data-testid="share-redeem-password"]',
    ) as HTMLInputElement | null;
    expect(passwordField).not.toBeNull();

    act(() => {
      if (passwordField) {
        passwordField.value = "secret";
        passwordField.dispatchEvent(new Event("input", { bubbles: true }));
        passwordField.dispatchEvent(new Event("change", { bubbles: true }));
      }
    });

    await act(async () => {
      await Promise.resolve();
    });

    const submitButton = container.querySelector(
      '[data-testid="share-redeem-submit"]',
    ) as HTMLButtonElement | null;
    expect(submitButton).not.toBeNull();

    act(() => {
      submitButton?.click();
    });

    await act(async () => {
      await Promise.resolve();
    });

    expect(mockRedeemShareLink).toHaveBeenNthCalledWith(1, {
      token: "token-2",
    });
    expect(mockRedeemShareLink).toHaveBeenNthCalledWith(2, {
      token: "token-2",
      password: "secret",
    });
    expect(mockNavigate).toHaveBeenCalledWith("/workspace/ws-2", {
      replace: true,
    });

    act(() => {
      root.unmount();
    });
  });

  it("shows a user-facing error for disabled links", async () => {
    mockRedeemShareLink.mockRejectedValue(
      new ApiClientError(
        400,
        "POST",
        "https://relay.scriptum.dev/v1/share-links/redeem",
        "SHARE_LINK_DISABLED",
        "disabled",
        false,
        null,
        null,
      ),
    );

    const { container, root } = renderRoute("/share/token-3");

    await act(async () => {
      await Promise.resolve();
    });

    const error = container.querySelector('[data-testid="share-redeem-error"]');
    expect(error?.textContent).toContain("disabled");

    act(() => {
      root.unmount();
    });
  });
});
