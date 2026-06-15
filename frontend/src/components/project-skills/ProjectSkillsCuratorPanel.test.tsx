import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { projectSkillsApi, type ProjectSkill } from "@/api/project-skills";

import { ProjectSkillsCuratorPanel } from "./ProjectSkillsCuratorPanel";

vi.mock("@/api/project-skills", () => ({
  projectSkillsApi: {
    list: vi.fn(),
    approve: vi.fn(),
    reject: vi.fn(),
    archive: vi.fn(),
    distill: vi.fn(),
  },
}));

const mockedProjectSkillsApi = vi.mocked(projectSkillsApi);

function stagedSkill(overrides: Partial<ProjectSkill> = {}): ProjectSkill {
  return {
    id: "skill-1",
    projectId: "project-1",
    title: "Check merge validation",
    bucket: "merge",
    stage: "review",
    status: "staged",
    pinned: false,
    archived: false,
    scopePaths: ["src-tauri"],
    compactGuidance: "Check validation failures before approving merge results.",
    bodyMarkdown: "Detailed skill guidance.",
    predictedEffect: "Prevents repeated validation loops.",
    provenance: { outcome_id: "outcome-1" },
    companionOfSkillId: null,
    createdAt: "2026-06-14T10:00:00Z",
    updatedAt: "2026-06-14T10:00:00Z",
    ...overrides,
  };
}

function renderPanel(projectId = "project-1") {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });

  const view = render(
    <QueryClientProvider client={queryClient}>
      <ProjectSkillsCuratorPanel projectId={projectId} />
    </QueryClientProvider>,
  );

  return { queryClient, ...view };
}

describe("ProjectSkillsCuratorPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedProjectSkillsApi.list.mockResolvedValue([stagedSkill()]);
    mockedProjectSkillsApi.approve.mockResolvedValue(
      stagedSkill({ status: "approved" }),
    );
    mockedProjectSkillsApi.reject.mockResolvedValue(
      stagedSkill({ status: "rejected" }),
    );
    mockedProjectSkillsApi.archive.mockResolvedValue(
      stagedSkill({ status: "archived", archived: true }),
    );
    mockedProjectSkillsApi.distill.mockResolvedValue({
      stagedSkills: [stagedSkill({ id: "skill-2" })],
      skippedExisting: 0,
    });
  });

  it("renders staged learned skills with review metadata", async () => {
    renderPanel();

    expect(
      await screen.findByText("Check merge validation"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Check validation failures before approving merge results."),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Prevents repeated validation loops."),
    ).toBeInTheDocument();
    expect(screen.getByText("merge")).toBeInTheDocument();
    expect(screen.getByText("review")).toBeInTheDocument();
    expect(mockedProjectSkillsApi.list).toHaveBeenCalledWith({
      projectId: "project-1",
      status: "staged",
      includeArchived: false,
    });
  });

  it("distills eligible outcomes and refreshes staged skills", async () => {
    mockedProjectSkillsApi.list
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([
        stagedSkill({ id: "skill-2", title: "Review PR evidence" }),
      ]);

    renderPanel();

    expect(await screen.findByText("No staged learned skills.")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /distill/i }));

    await waitFor(() => {
      expect(mockedProjectSkillsApi.distill).toHaveBeenCalledWith({
        projectId: "project-1",
        limit: 10,
      });
    });
    expect(await screen.findByText("Review PR evidence")).toBeInTheDocument();
  });

  it("runs lifecycle actions for staged skills", async () => {
    renderPanel();

    expect(await screen.findByText("Check merge validation")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /approve/i }));
    await waitFor(() => {
      expect(mockedProjectSkillsApi.approve).toHaveBeenCalledWith("skill-1");
    });

    fireEvent.click(screen.getByRole("button", { name: /reject/i }));
    await waitFor(() => {
      expect(mockedProjectSkillsApi.reject).toHaveBeenCalledWith("skill-1");
    });

    fireEvent.click(screen.getByRole("button", { name: /archive/i }));
    await waitFor(() => {
      expect(mockedProjectSkillsApi.archive).toHaveBeenCalledWith("skill-1");
    });
  });

  it("shows API failures without hiding the panel controls", async () => {
    mockedProjectSkillsApi.list.mockRejectedValueOnce(new Error("Scope rejected"));

    renderPanel();

    expect(await screen.findByRole("alert")).toHaveTextContent("Scope rejected");
    expect(screen.getByRole("button", { name: /distill/i })).toBeEnabled();
  });
});
