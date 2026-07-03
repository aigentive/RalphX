import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { GitHubConnectionStatus } from "@/api/github";

import { GitHubIntegrationSettingsPanel } from "./GitHubIntegrationSettingsPanel";

const githubStatusHook = vi.hoisted(() => ({
  refetch: vi.fn(),
  state: {
    data: null as GitHubConnectionStatus | null,
    error: null as Error | null,
    isError: false,
    isLoading: false,
    isFetching: false,
  },
}));

vi.mock("@/hooks/useGitHubConnectionStatus", () => ({
  useGitHubConnectionStatus: () => ({
    ...githubStatusHook.state,
    refetch: githubStatusHook.refetch,
  }),
}));

function renderPanel() {
  return render(<GitHubIntegrationSettingsPanel />);
}

describe("GitHubIntegrationSettingsPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    githubStatusHook.state.data = {
      ghInstalled: true,
      authenticated: true,
      host: "github.com",
      account: "octocat",
    };
    githubStatusHook.state.error = null;
    githubStatusHook.state.isError = false;
    githubStatusHook.state.isLoading = false;
    githubStatusHook.state.isFetching = false;
    githubStatusHook.refetch.mockResolvedValue({});
  });

  it("renders authenticated gh status without a token field", () => {
    renderPanel();

    expect(screen.getByText("GitHub")).toBeInTheDocument();
    expect(screen.getByText("GitHub CLI authenticated")).toBeInTheDocument();
    expect(screen.getByText("Host github.com")).toBeInTheDocument();
    expect(screen.getByText("Account octocat")).toBeInTheDocument();
    expect(screen.queryByLabelText(/token/i)).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  });

  it("shows gh auth login guidance when gh is installed but unauthenticated", () => {
    githubStatusHook.state.data = {
      ghInstalled: true,
      authenticated: false,
      host: "github.com",
      account: null,
    };

    renderPanel();

    expect(screen.getByText("GitHub CLI not authenticated")).toBeInTheDocument();
    expect(screen.getByText("gh auth login")).toBeInTheDocument();
    expect(screen.getByText("gh installed")).toBeInTheDocument();
    expect(screen.getByText("Not authenticated")).toBeInTheDocument();
  });

  it("distinguishes missing gh from an unauthenticated gh install", () => {
    githubStatusHook.state.data = {
      ghInstalled: false,
      authenticated: false,
      host: null,
      account: null,
    };

    renderPanel();

    expect(screen.getByText("GitHub CLI unavailable")).toBeInTheDocument();
    expect(screen.getByText("gh missing")).toBeInTheDocument();
    expect(screen.getByText("Host unknown")).toBeInTheDocument();
  });

  it("refreshes the live status on demand", async () => {
    const user = userEvent.setup();
    renderPanel();

    await user.click(screen.getByRole("button", { name: "Refresh" }));

    expect(githubStatusHook.refetch).toHaveBeenCalledTimes(1);
  });
});
