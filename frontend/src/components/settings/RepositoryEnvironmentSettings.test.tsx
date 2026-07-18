import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { RepositoryEnvironmentSettings } from "./RepositoryEnvironmentSettings";

const mocks = vi.hoisted(() => ({
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
  updateSettings: vi.fn(),
  settings: { removeInheritedGithubCliTokens: true },
  isLoading: false,
  isPending: false,
}));

vi.mock("sonner", () => ({
  toast: { success: mocks.toastSuccess, error: mocks.toastError },
}));

vi.mock("@/hooks/useRepositorySettings", () => ({
  useRepositorySettings: () => ({
    data: mocks.settings,
    isLoading: mocks.isLoading,
  }),
  useUpdateRepositorySettings: () => ({
    mutateAsync: mocks.updateSettings,
    isPending: mocks.isPending,
  }),
}));

describe("RepositoryEnvironmentSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.settings = { removeInheritedGithubCliTokens: true };
    mocks.isLoading = false;
    mocks.isPending = false;
    mocks.updateSettings.mockResolvedValue({ removeInheritedGithubCliTokens: false });
  });

  it("shows the safe default and explains the GitHub CLI precedence", () => {
    render(<RepositoryEnvironmentSettings />);

    expect(screen.getByText("Environment")).toBeInTheDocument();
    expect(screen.getByText("Remove inherited GitHub tokens")).toBeInTheDocument();
    expect(screen.getByTestId("remove-inherited-github-cli-tokens"))
      .toHaveAttribute("data-state", "checked");
    expect(screen.getByText(/overriding credentials saved by gh auth login/i))
      .toBeInTheDocument();
  });

  it("persists an explicit opt-out and warns that it affects new processes", async () => {
    const user = userEvent.setup();
    render(<RepositoryEnvironmentSettings />);

    await user.click(screen.getByTestId("remove-inherited-github-cli-tokens"));

    expect(mocks.updateSettings).toHaveBeenCalledWith({
      removeInheritedGithubCliTokens: false,
    });
    expect(mocks.toastSuccess).toHaveBeenCalledWith(
      "Inherited GitHub tokens will be passed to new processes",
    );
  });

  it("reports persistence failures without claiming the setting changed", async () => {
    mocks.updateSettings.mockRejectedValueOnce(new Error("database unavailable"));
    const user = userEvent.setup();
    render(<RepositoryEnvironmentSettings />);

    await user.click(screen.getByTestId("remove-inherited-github-cli-tokens"));

    expect(mocks.toastError).toHaveBeenCalledWith("database unavailable");
    expect(mocks.toastSuccess).not.toHaveBeenCalled();
  });
});
