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
    pin: vi.fn(),
    unpin: vi.fn(),
    previewExport: vi.fn(),
    applyExport: vi.fn(),
    getSettings: vi.fn(),
    updateSettings: vi.fn(),
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
    mockedProjectSkillsApi.list.mockImplementation((input) =>
      Promise.resolve(
        input.status === "approved"
          ? [
              stagedSkill({
                id: "approved-skill",
                title: "Approved review convention",
                compactGuidance: "Keep approved reviewer behavior reusable.",
                status: "approved",
              }),
            ]
          : [stagedSkill()],
      ),
    );
    mockedProjectSkillsApi.approve.mockResolvedValue(
      stagedSkill({ status: "approved" }),
    );
    mockedProjectSkillsApi.reject.mockResolvedValue(
      stagedSkill({ status: "rejected" }),
    );
    mockedProjectSkillsApi.archive.mockResolvedValue(
      stagedSkill({ status: "archived", archived: true }),
    );
    mockedProjectSkillsApi.pin.mockResolvedValue(
      stagedSkill({ id: "approved-skill", status: "approved", pinned: true }),
    );
    mockedProjectSkillsApi.unpin.mockResolvedValue(
      stagedSkill({ id: "approved-skill", status: "approved", pinned: false }),
    );
    mockedProjectSkillsApi.getSettings.mockResolvedValue({
      projectId: "project-1",
      exportEnabled: false,
    });
    mockedProjectSkillsApi.updateSettings.mockResolvedValue({
      projectId: "project-1",
      exportEnabled: true,
    });
    mockedProjectSkillsApi.previewExport.mockResolvedValue({
      projectId: "project-1",
      targetRoot: "/repo/.claude/skills",
      count: 1,
      files: [
        {
          projectSkillId: "approved-skill",
          title: "Approved review convention",
          relativePath: ".claude/skills/approved-review-convention/SKILL.md",
          pinned: false,
          status: "approved",
          willWrite: true,
        },
      ],
    });
    mockedProjectSkillsApi.applyExport.mockResolvedValue({
      projectId: "project-1",
      targetRoot: "/repo/.claude/skills",
      count: 1,
      files: [
        {
          projectSkillId: "approved-skill",
          title: "Approved review convention",
          relativePath: ".claude/skills/approved-review-convention/SKILL.md",
          pinned: false,
          status: "approved",
          willWrite: false,
        },
      ],
    });
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
    expect(screen.getAllByText("merge").length).toBeGreaterThan(0);
    expect(screen.getByText("review")).toBeInTheDocument();
    expect(mockedProjectSkillsApi.list).toHaveBeenCalledWith({
      projectId: "project-1",
      status: "staged",
      includeArchived: false,
    });
    expect(mockedProjectSkillsApi.list).toHaveBeenCalledWith({
      projectId: "project-1",
      status: "approved",
      includeArchived: false,
    });
  });

  it("distills eligible outcomes and refreshes staged skills", async () => {
    mockedProjectSkillsApi.list.mockImplementation((input) =>
      Promise.resolve(
        input.status === "approved"
          ? []
          : mockedProjectSkillsApi.distill.mock.calls.length > 0
            ? [stagedSkill({ id: "skill-2", title: "Review PR evidence" })]
            : [],
      ),
    );

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

    fireEvent.click(screen.getAllByRole("button", { name: /archive/i })[0]);
    await waitFor(() => {
      expect(mockedProjectSkillsApi.archive).toHaveBeenCalledWith("skill-1");
    });
  });

  it("pins approved skills", async () => {
    mockedProjectSkillsApi.list.mockImplementation((input) =>
      Promise.resolve(
        input.status === "approved"
          ? [
              stagedSkill({
                id: "approved-skill",
                title: "Approved review convention",
                status: "approved",
              }),
            ]
          : [],
      ),
    );

    renderPanel();

    expect(await screen.findByText("Approved review convention")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^pin$/i }));
    await waitFor(() => {
      expect(mockedProjectSkillsApi.pin).toHaveBeenCalledWith("approved-skill");
    });
  });

  it("unpins approved skills", async () => {
    mockedProjectSkillsApi.list.mockImplementation((input) =>
      Promise.resolve(
        input.status === "approved"
          ? [
              stagedSkill({
                id: "approved-skill",
                title: "Pinned review convention",
                status: "approved",
                pinned: true,
              }),
            ]
          : [],
      ),
    );

    renderPanel();

    expect(await screen.findByText("Pinned review convention")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^unpin$/i }));
    await waitFor(() => {
      expect(mockedProjectSkillsApi.unpin).toHaveBeenCalledWith("approved-skill");
    });
  });

  it("previews and applies approved skill export after opt-in", async () => {
    renderPanel();

    expect(await screen.findByText("Approved review convention")).toBeInTheDocument();
    const exportButton = screen.getByRole("button", { name: /^export$/i });
    expect(exportButton).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: /preview export/i }));
    await waitFor(() => {
      expect(mockedProjectSkillsApi.previewExport).toHaveBeenCalledWith(
        "project-1",
      );
    });
    expect(await screen.findByText(/1 pending file/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("switch", { name: /enable project skill export/i }));
    await waitFor(() => {
      expect(mockedProjectSkillsApi.updateSettings).toHaveBeenCalledWith(
        "project-1",
        { exportEnabled: true },
      );
    });
    mockedProjectSkillsApi.getSettings.mockResolvedValue({
      projectId: "project-1",
      exportEnabled: true,
    });
    await waitFor(() => {
      expect(exportButton).toBeEnabled();
    });

    fireEvent.click(screen.getByRole("button", { name: /^export$/i }));
    await waitFor(() => {
      expect(mockedProjectSkillsApi.applyExport).toHaveBeenCalledWith("project-1");
    });
    expect(await screen.findByText(/0 pending files/)).toBeInTheDocument();
  });

  it("shows API failures without hiding the panel controls", async () => {
    mockedProjectSkillsApi.list.mockRejectedValueOnce(new Error("Scope rejected"));

    renderPanel();

    expect(await screen.findByRole("alert")).toHaveTextContent("Scope rejected");
    expect(screen.getByRole("button", { name: /distill/i })).toBeEnabled();
  });
});
