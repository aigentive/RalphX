import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { GitHubRepoPicker } from "./GitHubRepoPicker";
import { projectsApi } from "@/api/projects";
import { createTestQueryClient } from "@/test/store-utils";

const mockAuth = vi.hoisted(() => ({ isAuthenticated: false }));

vi.mock("@/hooks/useGithubSettings", () => ({
  useGhAuthStatus: () => ({ data: mockAuth.isAuthenticated, isLoading: false }),
}));

vi.mock("@/api/projects", () => ({
  projectsApi: { listGithubRepositories: vi.fn() },
}));

const mockListGithubRepositories = vi.mocked(projectsApi.listGithubRepositories);

function renderPicker(onSelectRepo = vi.fn()) {
  const queryClient = createTestQueryClient();
  return {
    onSelectRepo,
    ...render(
      <QueryClientProvider client={queryClient}>
        <GitHubRepoPicker onSelectRepo={onSelectRepo} />
      </QueryClientProvider>
    ),
  };
}

describe("GitHubRepoPicker", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockAuth.isAuthenticated = false;
  });

  it("renders nothing when unauthenticated", () => {
    renderPicker();
    expect(screen.queryByTestId("github-repo-picker-trigger")).not.toBeInTheDocument();
    expect(mockListGithubRepositories).not.toHaveBeenCalled();
  });

  it("does not fetch on mount, only on expand", async () => {
    mockAuth.isAuthenticated = true;
    mockListGithubRepositories.mockResolvedValue([]);
    renderPicker();

    expect(mockListGithubRepositories).not.toHaveBeenCalled();

    const user = userEvent.setup();
    await user.click(screen.getByTestId("github-repo-picker-trigger"));

    await waitFor(() => expect(mockListGithubRepositories).toHaveBeenCalledTimes(1));
  });

  it("fills the URL with owner/repo shorthand on selection", async () => {
    mockAuth.isAuthenticated = true;
    mockListGithubRepositories.mockResolvedValue([
      { nameWithOwner: "acme/demo", description: null, isPrivate: false, updatedAt: null },
    ]);
    const { onSelectRepo } = renderPicker();
    const user = userEvent.setup();

    await user.click(screen.getByTestId("github-repo-picker-trigger"));
    await waitFor(() => expect(screen.getByTestId("github-repo-picker-item")).toBeInTheDocument());
    await user.click(screen.getByTestId("github-repo-picker-item"));

    expect(onSelectRepo).toHaveBeenCalledWith("acme/demo");
  });

  it("falls back silently to plain URL entry on a gh failure - no error banner", async () => {
    mockAuth.isAuthenticated = true;
    mockListGithubRepositories.mockRejectedValue(new Error("gh: not authenticated"));
    renderPicker();
    const user = userEvent.setup();

    await user.click(screen.getByTestId("github-repo-picker-trigger"));

    await waitFor(() => {
      expect(screen.getByTestId("github-repo-picker-trigger")).toHaveAttribute(
        "data-state",
        "closed"
      );
    });
    expect(screen.queryByText(/gh: not authenticated/i)).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
