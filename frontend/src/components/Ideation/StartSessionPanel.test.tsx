/**
 * StartSessionPanel component tests — team flow only
 *
 * Tests mode selector, TeamConfigPanel reveal, start handlers
 * (solo vs team paths), seed-from-task with team params, and error handling.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StartSessionPanel } from "./StartSessionPanel";

// Mock dependencies
const mockMutateAsync = vi.fn();
vi.mock("@/hooks/useIdeation", () => ({
  useCreateIdeationSession: () => ({
    mutateAsync: mockMutateAsync,
    isPending: false,
  }),
}));

const mockAddSession = vi.fn();
const mockSetActiveSession = vi.fn();
vi.mock("@/stores/ideationStore", () => ({
  useIdeationStore: (selector: (s: Record<string, unknown>) => unknown) =>
    selector({
      addSession: mockAddSession,
      setActiveSession: mockSetActiveSession,
    }),
}));

let mockActiveProjectId: string | null = "project-1";
vi.mock("@/stores/projectStore", () => ({
  useProjectStore: (selector: (s: Record<string, unknown>) => unknown) =>
    selector({
      activeProjectId: mockActiveProjectId,
      projects: {
        "project-1": {
          id: "project-1",
          name: "Mock Project",
          workingDirectory: "/mock/repo",
          baseBranch: "main",
        },
      },
    }),
}));

const mockGetGitDefaultBranch = vi.fn();
const mockGetGitCurrentBranch = vi.fn();
const mockGetGitBranches = vi.fn();
const mockSearchGithubPullRequests = vi.fn();
const mockGetPlanBranches = vi.fn();
const mockListIdeationSessions = vi.fn();
const mockListConversations = vi.fn();
const mockListAgentConversationWorkspacesByProject = vi.fn();
vi.mock("@/api/projects", () => ({
  getGitDefaultBranch: (...args: unknown[]) => mockGetGitDefaultBranch(...args),
  getGitCurrentBranch: (...args: unknown[]) => mockGetGitCurrentBranch(...args),
  getGitBranches: (...args: unknown[]) => mockGetGitBranches(...args),
  searchGithubPullRequests: (...args: unknown[]) => mockSearchGithubPullRequests(...args),
}));
vi.mock("@/api/plan-branch", () => ({
  planBranchApi: {
    getByProject: (...args: unknown[]) => mockGetPlanBranches(...args),
  },
}));
vi.mock("@/api/ideation", () => ({
  ideationApi: {
    sessions: {
      list: (...args: unknown[]) => mockListIdeationSessions(...args),
    },
  },
}));
vi.mock("@/api/chat", () => ({
  chatApi: {
    listConversations: (...args: unknown[]) => mockListConversations(...args),
    listAgentConversationWorkspacesByProject: (...args: unknown[]) =>
      mockListAgentConversationWorkspacesByProject(...args),
  },
}));

// Mock sonner toast
const mockToastError = vi.fn();
vi.mock("sonner", () => ({
  toast: { error: (...args: unknown[]) => mockToastError(...args) },
}));

// Mock useSessionExportImport
const mockImportSession = vi.fn();
vi.mock("@/hooks/useSessionExportImport", () => ({
  useSessionExportImport: () => ({
    importSession: mockImportSession,
    exportSession: vi.fn(),
    isImporting: false,
    isExporting: false,
  }),
}));

let mockIdeationTeamModeAvailable = true;
vi.mock("@/hooks/useTeamModeAvailability", () => ({
  useTeamModeAvailability: () => ({
    ideationTeamModeAvailable: mockIdeationTeamModeAvailable,
  }),
}));

// Mock TaskPickerDialog to avoid complex rendering
vi.mock("./TaskPickerDialog", () => ({
  TaskPickerDialog: ({ isOpen, onSelect }: { isOpen: boolean; onClose: () => void; onSelect: (task: unknown) => void }) =>
    isOpen ? (
      <div data-testid="task-picker">
        <button data-testid="pick-task" onClick={() => onSelect({ id: "task-1", projectId: "project-1", title: "Mock Task" })}>
          Pick
        </button>
      </div>
    ) : null,
}));

async function expectStartFromLabel(label: string) {
  await waitFor(() => {
    expect(screen.getByTestId("start-from-select")).toHaveTextContent(label);
  });
}

describe("StartSessionPanel", () => {
  const onNewSession = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    mockActiveProjectId = "project-1";
    mockMutateAsync.mockResolvedValue({ id: "session-1", projectId: "project-1" });
    mockIdeationTeamModeAvailable = true;
    mockGetGitDefaultBranch.mockResolvedValue("main");
    mockGetGitCurrentBranch.mockResolvedValue("main");
    mockGetGitBranches.mockResolvedValue(["main", "feature/mock"]);
    mockSearchGithubPullRequests.mockResolvedValue([]);
    mockGetPlanBranches.mockResolvedValue([]);
    mockListIdeationSessions.mockResolvedValue([]);
    mockListConversations.mockResolvedValue([]);
    mockListAgentConversationWorkspacesByProject.mockResolvedValue([]);
  });

  describe("mode selector", () => {
    it("renders Solo, Research Team, and Debate Team buttons", () => {
      render(<StartSessionPanel onNewSession={onNewSession} />);
      expect(screen.getByText("Solo")).toBeInTheDocument();
      expect(screen.getByText(/Research Team/)).toBeInTheDocument();
      expect(screen.getByText(/Debate Team/)).toBeInTheDocument();
    });

    it("defaults to solo mode", () => {
      render(<StartSessionPanel onNewSession={onNewSession} />);
      expect(screen.getByText("Start New Session")).toBeInTheDocument();
    });

    it("switches button text when Research Team selected", () => {
      render(<StartSessionPanel onNewSession={onNewSession} />);
      fireEvent.click(screen.getByText(/Research Team/));
      expect(screen.getByText("Start Research Session")).toBeInTheDocument();
    });

    it("switches button text when Debate Team selected", () => {
      render(<StartSessionPanel onNewSession={onNewSession} />);
      fireEvent.click(screen.getByText(/Debate Team/));
      expect(screen.getByText("Start Debate Session")).toBeInTheDocument();
    });

    it("hides team mode controls when team mode is unavailable", () => {
      mockIdeationTeamModeAvailable = false;

      render(<StartSessionPanel onNewSession={onNewSession} />);

      expect(screen.queryByText("Ideation Mode")).not.toBeInTheDocument();
      expect(screen.queryByText(/Research Team/)).not.toBeInTheDocument();
      expect(screen.queryByText(/Debate Team/)).not.toBeInTheDocument();
      expect(screen.getByText("Start New Session")).toBeInTheDocument();
    });
  });

  describe("TeamConfigPanel animated reveal", () => {
    it("hides config panel in solo mode (maxHeight 0)", () => {
      const { container } = render(<StartSessionPanel onNewSession={onNewSession} />);
      const wrapper = container.querySelector(".overflow-hidden.transition-all");
      expect(wrapper).toHaveStyle({ maxHeight: "0px", opacity: "0" });
    });

    it("reveals config panel in team mode (maxHeight 280px)", () => {
      const { container } = render(<StartSessionPanel onNewSession={onNewSession} />);
      fireEvent.click(screen.getByText(/Research Team/));
      const wrapper = container.querySelector(".overflow-hidden.transition-all");
      expect(wrapper).toHaveStyle({ maxHeight: "280px", opacity: "1" });
    });
  });

  describe("handleStartSession", () => {
    it("calls onNewSession in solo mode (via ⌘N shortcut)", () => {
      render(<StartSessionPanel onNewSession={onNewSession} />);
      // ⌘N keyboard shortcut triggers onNewSession when in solo mode
      fireEvent.keyDown(window, { key: "n", metaKey: true });
      expect(onNewSession).toHaveBeenCalledOnce();
      expect(mockMutateAsync).not.toHaveBeenCalled();
    });

    it("calls createSession.mutateAsync in team mode", async () => {
      render(<StartSessionPanel onNewSession={onNewSession} />);
      await expectStartFromLabel("Project default (main)");
      fireEvent.click(screen.getByText(/Research Team/));

      await act(async () => {
        fireEvent.click(screen.getByText("Start Research Session"));
      });

      expect(mockMutateAsync).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-1",
          analysisBase: expect.objectContaining({
            kind: "project_default",
            ref: "main",
          }),
          teamMode: "research",
          teamConfig: expect.objectContaining({ maxTeammates: 5 }),
        }),
      );
    });

    it("starts team ideation from a selected pull request head branch", async () => {
      const user = userEvent.setup();
      mockSearchGithubPullRequests.mockResolvedValueOnce([
        {
          number: 42,
          title: "Add PR picker",
          url: "https://github.com/owner/repo/pull/42",
          headRefName: "feature/pr-picker",
          headRefOid: "abc123",
          baseRefName: "main",
          isDraft: false,
          updatedAt: "2026-05-21T10:00:00Z",
          authorLogin: "dev",
          isCrossRepository: false,
        },
      ]);

      render(<StartSessionPanel onNewSession={onNewSession} />);
      await expectStartFromLabel("Project default (main)");

      await user.click(screen.getByTestId("start-from-select"));
      await user.click(screen.getByRole("tab", { name: /PRs/i }));
      await waitFor(() =>
        expect(mockSearchGithubPullRequests).toHaveBeenCalledWith({
          projectId: "project-1",
          query: "",
          limit: 30,
        })
      );
      await user.click(await screen.findByText("#42 Add PR picker"));
      fireEvent.click(screen.getByText(/Research Team/));

      await act(async () => {
        fireEvent.click(screen.getByText("Start Research Session"));
      });

      expect(mockMutateAsync).toHaveBeenCalledWith(
        expect.objectContaining({
          analysisBase: expect.objectContaining({
            kind: "local_branch",
            ref: "feature/pr-picker",
            displayName: "PR #42: Add PR picker",
            sourcePullRequest: expect.objectContaining({
              number: 42,
              headRefName: "feature/pr-picker",
              baseRefName: "main",
              headRefOid: "abc123",
            }),
          }),
        }),
      );
    });

    it("keeps a selected pull request available across later searches", async () => {
      const user = userEvent.setup();
      mockSearchGithubPullRequests
        .mockResolvedValueOnce([
          {
            number: 42,
            title: "Add PR picker",
            url: "https://github.com/owner/repo/pull/42",
            headRefName: "feature/pr-picker",
            headRefOid: "abc123",
            baseRefName: "main",
            isDraft: false,
            updatedAt: "2026-05-21T10:00:00Z",
            authorLogin: "dev",
            isCrossRepository: false,
          },
        ])
        .mockResolvedValueOnce([]);

      render(<StartSessionPanel onNewSession={onNewSession} />);
      await expectStartFromLabel("Project default (main)");

      await user.click(screen.getByTestId("start-from-select"));
      await user.click(screen.getByRole("tab", { name: /PRs/i }));
      await user.click(await screen.findByText("#42 Add PR picker"));
      await expectStartFromLabel("#42 Add PR picker");

      await user.click(screen.getByTestId("start-from-select"));
      await user.click(screen.getByRole("tab", { name: /PRs/i }));

      await waitFor(() => expect(mockSearchGithubPullRequests).toHaveBeenCalledTimes(2));
      expect(screen.getAllByText("#42 Add PR picker").length).toBeGreaterThan(1);
    });

    it("shows pull request search errors", async () => {
      const user = userEvent.setup();
      mockSearchGithubPullRequests.mockRejectedValueOnce(new Error("GitHub search failed"));

      render(<StartSessionPanel onNewSession={onNewSession} />);
      await expectStartFromLabel("Project default (main)");

      await user.click(screen.getByTestId("start-from-select"));
      await user.click(screen.getByRole("tab", { name: /PRs/i }));

      expect(await screen.findByText("GitHub search failed")).toBeInTheDocument();
    });

    it("adds session to store and sets active on success", async () => {
      render(<StartSessionPanel onNewSession={onNewSession} />);
      await expectStartFromLabel("Project default (main)");
      fireEvent.click(screen.getByText(/Research Team/));

      await act(async () => {
        fireEvent.click(screen.getByText("Start Research Session"));
      });

      expect(mockAddSession).toHaveBeenCalledWith({ id: "session-1", projectId: "project-1" });
      expect(mockSetActiveSession).toHaveBeenCalledWith("session-1");
    });

    it("shows toast error when no active project in team mode", async () => {
      mockActiveProjectId = null;
      render(<StartSessionPanel onNewSession={onNewSession} />);
      fireEvent.click(screen.getByText(/Debate Team/));

      await act(async () => {
        fireEvent.click(screen.getByText("Start Debate Session"));
      });

      expect(mockToastError).toHaveBeenCalledWith("No active project selected");
      expect(mockMutateAsync).not.toHaveBeenCalled();
    });

    it("shows toast error on createSession failure", async () => {
      mockMutateAsync.mockRejectedValueOnce(new Error("API error"));
      render(<StartSessionPanel onNewSession={onNewSession} />);
      await expectStartFromLabel("Project default (main)");
      fireEvent.click(screen.getByText(/Research Team/));

      await act(async () => {
        fireEvent.click(screen.getByText("Start Research Session"));
      });

      expect(mockToastError).toHaveBeenCalledWith("Failed to create session");
    });
  });

  describe("handleSeedFromTask", () => {
    it("preselects current branch when it differs from project default", async () => {
      mockGetGitCurrentBranch.mockResolvedValueOnce("feature/current");
      mockGetGitBranches.mockResolvedValueOnce(["main", "feature/current", "feature/other"]);

      render(<StartSessionPanel onNewSession={onNewSession} />);

      await expectStartFromLabel("Current branch (feature/current)");
      fireEvent.click(screen.getByTestId("start-from-select"));
      expect(await screen.findByText("Project default (main)")).toBeInTheDocument();
    });

    it("includes team params when in team mode", async () => {
      render(<StartSessionPanel onNewSession={onNewSession} />);
      await expectStartFromLabel("Project default (main)");
      fireEvent.click(screen.getByText(/Debate Team/));

      // Open task picker via secondary button
      fireEvent.click(screen.getByText("Seed from Draft Task"));

      await waitFor(() => {
        expect(screen.getByTestId("task-picker")).toBeInTheDocument();
      });

      await act(async () => {
        fireEvent.click(screen.getByTestId("pick-task"));
      });

      expect(mockMutateAsync).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-1",
          title: "Ideation: Mock Task",
          seedTaskId: "task-1",
          analysisBase: expect.objectContaining({
            kind: "project_default",
            ref: "main",
          }),
          teamMode: "debate",
          teamConfig: expect.objectContaining({ compositionMode: "dynamic" }),
        }),
      );
    });

    it("omits team params when in solo mode", async () => {
      render(<StartSessionPanel onNewSession={onNewSession} />);
      await expectStartFromLabel("Project default (main)");

      fireEvent.click(screen.getByText("Seed from Draft Task"));

      await waitFor(() => {
        expect(screen.getByTestId("task-picker")).toBeInTheDocument();
      });

      await act(async () => {
        fireEvent.click(screen.getByTestId("pick-task"));
      });

      expect(mockMutateAsync).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-1",
          seedTaskId: "task-1",
        }),
      );
      // Should NOT have team params
      const callArgs = mockMutateAsync.mock.calls[0]![0];
      expect(callArgs.teamMode).toBeUndefined();
      expect(callArgs.teamConfig).toBeUndefined();
      expect(callArgs.analysisBase).toEqual(
        expect.objectContaining({ kind: "project_default", ref: "main" }),
      );
    });

    it("shows toast error on seed failure", async () => {
      mockMutateAsync.mockRejectedValueOnce(new Error("Seed fail"));
      render(<StartSessionPanel onNewSession={onNewSession} />);
      await expectStartFromLabel("Project default (main)");

      fireEvent.click(screen.getByText("Seed from Draft Task"));

      await waitFor(() => {
        expect(screen.getByTestId("task-picker")).toBeInTheDocument();
      });

      await act(async () => {
        fireEvent.click(screen.getByTestId("pick-task"));
      });

      expect(mockToastError).toHaveBeenCalledWith("Failed to start ideation session");
    });
  });

  describe("import session button", () => {
    it("renders the Import Session button", () => {
      render(<StartSessionPanel onNewSession={onNewSession} />);
      expect(screen.getByText("Import Session")).toBeInTheDocument();
    });

    it("calls importSession with activeProjectId when Import Session is clicked", async () => {
      mockImportSession.mockResolvedValueOnce(undefined);
      render(<StartSessionPanel onNewSession={onNewSession} />);

      await act(async () => {
        fireEvent.click(screen.getByText("Import Session"));
      });

      expect(mockImportSession).toHaveBeenCalledWith("project-1");
    });
  });
});
