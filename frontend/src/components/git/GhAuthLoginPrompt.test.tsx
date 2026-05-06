/**
 * Tests for GhAuthLoginPrompt — covers null-render gating, code-only render,
 * url-only render, both-rendered, and the openUrl click handler.
 */

import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { mocks } = vi.hoisted(() => ({
  mocks: {
    openUrl: vi.fn().mockResolvedValue(undefined),
  },
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: (...args: unknown[]) => mocks.openUrl(...args),
}));

import { GhAuthLoginPrompt } from "./GhAuthLoginPrompt";

beforeEach(() => {
  mocks.openUrl.mockReset();
  mocks.openUrl.mockResolvedValue(undefined);
});

describe("GhAuthLoginPrompt", () => {
  it("renders nothing when both code and url are missing", () => {
    const { container } = render(<GhAuthLoginPrompt prompt={{}} />);
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing when code and url are explicitly null", () => {
    const { container } = render(
      <GhAuthLoginPrompt prompt={{ code: null, url: null }} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders code block when only code is present", () => {
    render(<GhAuthLoginPrompt prompt={{ code: "ABCD-1234" }} />);
    expect(screen.getByTestId("gh-auth-login-prompt")).toBeInTheDocument();
    expect(screen.getByText("Enter this GitHub code:")).toBeInTheDocument();
    expect(screen.getByText("ABCD-1234")).toBeInTheDocument();
    expect(screen.queryByTestId("gh-auth-open-github")).toBeNull();
  });

  it("renders Open GitHub button when only url is present", () => {
    render(
      <GhAuthLoginPrompt
        prompt={{ url: "https://github.com/login/device" }}
      />,
    );
    expect(screen.getByTestId("gh-auth-login-prompt")).toBeInTheDocument();
    expect(screen.getByTestId("gh-auth-open-github")).toBeInTheDocument();
    expect(screen.queryByText("Enter this GitHub code:")).toBeNull();
  });

  it("renders both code and Open GitHub button when both fields are present", () => {
    render(
      <GhAuthLoginPrompt
        prompt={{
          code: "ABCD-1234",
          url: "https://github.com/login/device",
        }}
      />,
    );
    expect(screen.getByText("ABCD-1234")).toBeInTheDocument();
    expect(screen.getByTestId("gh-auth-open-github")).toBeInTheDocument();
  });

  it("clicking Open GitHub invokes openUrl with the prompt url", async () => {
    const user = userEvent.setup();
    render(
      <GhAuthLoginPrompt
        prompt={{ url: "https://github.com/login/device" }}
      />,
    );
    await user.click(screen.getByTestId("gh-auth-open-github"));
    expect(mocks.openUrl).toHaveBeenCalledWith(
      "https://github.com/login/device",
    );
  });
});
