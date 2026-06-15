import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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
    listReportCards: vi.fn(),
    previewImport: vi.fn(),
    applyImport: vi.fn(),
    promoteMemory: vi.fn(),
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
    mockedProjectSkillsApi.listReportCards.mockResolvedValue({
      count: 1,
      cards: [
        {
          projectSkillId: "approved-skill",
          title: "Approved review convention",
          bucket: "review",
          stage: "review",
          pinned: false,
          usageCount: 3,
          linkedOutcomeCount: 2,
          succeededOutcomeCount: 1,
          failedOutcomeCount: 1,
          unknownOutcomeCount: 0,
          lastUsedAt: "2026-06-15T10:00:00Z",
          ageDays: 1,
          agingStatus: "active",
          evidenceLevel: "insufficient_data",
        },
      ],
    });
    mockedProjectSkillsApi.previewImport.mockResolvedValue({
      rows: [
        {
          index: 0,
          externalId: "manifest-skill-1",
          title: "Imported review convention",
          decision: "eligible",
          reasons: [],
          duplicateProjectSkillId: null,
        },
      ],
      eligibleCount: 1,
      invalidCount: 0,
      duplicateCount: 0,
    });
    mockedProjectSkillsApi.applyImport.mockResolvedValue({
      preview: {
        rows: [
          {
            index: 0,
            externalId: "manifest-skill-1",
            title: "Imported review convention",
            decision: "eligible",
            reasons: [],
            duplicateProjectSkillId: null,
          },
        ],
        eligibleCount: 1,
        invalidCount: 0,
        duplicateCount: 0,
      },
      importedSkills: [stagedSkill({ id: "imported-skill" })],
      importedCount: 1,
    });
    mockedProjectSkillsApi.promoteMemory.mockResolvedValue(
      stagedSkill({ id: "promoted-skill", title: "Promoted review procedure" }),
    );
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
    expect(screen.getAllByText("review").length).toBeGreaterThan(0);
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
    expect(mockedProjectSkillsApi.listReportCards).toHaveBeenCalledWith({
      projectId: "project-1",
      minLinkedOutcomes: 5,
      staleAfterDays: 30,
    });
  });

  it("renders conservative report cards for approved skills", async () => {
    renderPanel();
    const user = userEvent.setup();

    await user.click(await screen.findByRole("tab", { name: /reports/i }));
    expect(await screen.findByText("low sample")).toBeInTheDocument();
    expect(screen.getAllByText("Approved review convention").length).toBeGreaterThan(0);
    expect(screen.getByText("3 uses")).toBeInTheDocument();
    expect(screen.getByText("2 linked outcomes")).toBeInTheDocument();
    expect(screen.getByText("1 succeeded")).toBeInTheDocument();
    expect(screen.getByText("1 failed")).toBeInTheDocument();
  });

  it("previews and applies project skill imports", async () => {
    renderPanel();
    const user = userEvent.setup();

    expect(screen.queryByText("Import draft skills")).not.toBeInTheDocument();
    await user.click(await screen.findByRole("tab", { name: /advanced/i }));
    await screen.findByText("Import draft skills");
    expect(
      screen.getByText(/Preview validates each row first/i),
    ).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Project skill import manifest"), {
      target: {
        value: JSON.stringify({
          candidates: [
            {
              externalId: "manifest-skill-1",
              title: "Imported review convention",
              bucket: "review",
              stage: "review",
              compactGuidance: "Check imported review convention.",
              bodyMarkdown: "Detailed guidance",
              predictedEffect: "Reduces review misses.",
              provenance: { source: "manifest" },
              sourceSnapshot: { kind: "project_skill_manifest" },
            },
          ],
        }),
      },
    });

    fireEvent.click(screen.getByRole("button", { name: /preview manifest/i }));
    await waitFor(() => {
      expect(mockedProjectSkillsApi.previewImport).toHaveBeenCalledWith(
        expect.objectContaining({ projectId: "project-1" }),
      );
    });
    expect(await screen.findByText("1 eligible")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /add to review queue/i }));
    await waitFor(() => {
      expect(mockedProjectSkillsApi.applyImport).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-1",
          confirmImport: true,
        }),
      );
    });
  });

  it("promotes memory entries through the curator panel", async () => {
    renderPanel();
    const user = userEvent.setup();

    expect(screen.queryByText("Draft from memory")).not.toBeInTheDocument();
    await user.click(await screen.findByRole("tab", { name: /advanced/i }));
    await screen.findByText("Draft from memory");
    expect(
      screen.getByText(/Use one saved memory only as provenance/i),
    ).toBeInTheDocument();
    expect(screen.getByText("Memory ID")).toBeInTheDocument();
    expect(screen.getByText("Compact guidance")).toBeInTheDocument();
    expect(screen.getByText("Skill body")).toBeInTheDocument();
    expect(screen.getByText("Predicted effect")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Memory id"), {
      target: { value: "memory-1" },
    });
    fireEvent.change(screen.getByLabelText("Promoted skill title"), {
      target: { value: "Promoted review procedure" },
    });
    fireEvent.change(screen.getByLabelText("Promoted skill guidance"), {
      target: { value: "Turn the memory into a review checklist." },
    });
    fireEvent.change(screen.getByLabelText("Promoted skill body"), {
      target: { value: "## Procedure\n\nApply the checklist." },
    });
    fireEvent.change(screen.getByLabelText("Promoted skill predicted effect"), {
      target: { value: "Reduces review misses." },
    });

    fireEvent.click(screen.getByRole("button", { name: /create draft skill/i }));
    await waitFor(() => {
      expect(mockedProjectSkillsApi.promoteMemory).toHaveBeenCalledWith({
        projectId: "project-1",
        memoryId: "memory-1",
        title: "Promoted review procedure",
        bucket: "review",
        stage: "review",
        compactGuidance: "Turn the memory into a review checklist.",
        bodyMarkdown: "## Procedure\n\nApply the checklist.",
        predictedEffect: "Reduces review misses.",
      });
    });
  });

  it("distills eligible outcomes and refreshes staged skills", async () => {
    const user = userEvent.setup();
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

    expect(
      screen.queryByText(/Scan stored task, conversation, and agent workspace outcomes/i),
    ).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /find candidates\.\.\./i }));
    expect(
      await screen.findByRole("dialog", { name: /find skill candidates/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Scan stored task, conversation, and agent workspace outcomes/i),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^find candidates$/i }));

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
    const user = userEvent.setup();

    await user.click(await screen.findByRole("tab", { name: /^approved$/i }));
    expect(
      (await screen.findAllByText("Approved review convention")).length,
    ).toBeGreaterThan(0);

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
    const user = userEvent.setup();

    await user.click(await screen.findByRole("tab", { name: /^approved$/i }));
    expect(await screen.findByText("Pinned review convention")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^unpin$/i }));
    await waitFor(() => {
      expect(mockedProjectSkillsApi.unpin).toHaveBeenCalledWith("approved-skill");
    });
  });

  it("previews and applies approved skill export after opt-in", async () => {
    renderPanel();
    const user = userEvent.setup();

    await user.click(await screen.findByRole("tab", { name: /^approved$/i }));
    expect(
      (await screen.findAllByText("Approved review convention")).length,
    ).toBeGreaterThan(0);
    expect(screen.queryByText(/Apply requires a clean git worktree/i)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /export\.\.\./i }));
    expect(
      await screen.findByRole("dialog", { name: /export approved skills/i }),
    ).toBeInTheDocument();
    const exportButton = screen.getByRole("button", { name: /^export$/i });
    expect(exportButton).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: /preview export/i }));
    await waitFor(() => {
      expect(mockedProjectSkillsApi.previewExport).toHaveBeenCalledWith(
        "project-1",
      );
    });
    expect(await screen.findByText(/1 pending file/)).toBeInTheDocument();
    expect(
      screen.getByText(/Apply requires a clean git worktree on a named review branch/i),
    ).toBeInTheDocument();
    expect(
      screen.getByText(".claude/skills/approved-review-convention/SKILL.md"),
    ).toBeInTheDocument();
    expect(screen.getByText("will write")).toBeInTheDocument();

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
    expect(screen.getByRole("button", { name: /find candidates/i })).toBeEnabled();
  });
});
