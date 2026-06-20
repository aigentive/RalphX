import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { projectsApi } from "@/api/projects";
import type { Project } from "@/types/project";
import { PrTemplateEditorDialog } from "./PrTemplateEditorDialog";

vi.mock("@/api/projects", () => ({
  projectsApi: {
    readPrTemplate: vi.fn(),
    writePrTemplate: vi.fn(),
  },
}));

const project: Project = {
  id: "project-1",
  name: "RalphX",
  workingDirectory: "/tmp/ralphx",
  gitMode: "worktree",
  baseBranch: null,
  worktreeParentDirectory: null,
  useFeatureBranches: true,
  mergeValidationMode: "off",
  detectedAnalysis: null,
  customAnalysis: null,
  analyzedAt: null,
  githubPrEnabled: true,
  createdAt: "2026-04-22T10:00:00Z",
  updatedAt: "2026-04-22T10:00:00Z",
};

function renderDialog(
  overrides: Partial<Parameters<typeof PrTemplateEditorDialog>[0]> = {},
) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const props = {
    open: true,
    onOpenChange: vi.fn(),
    project,
    ...overrides,
  };

  render(
    <QueryClientProvider client={queryClient}>
      <PrTemplateEditorDialog {...props} />
    </QueryClientProvider>,
  );

  return props;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

describe("PrTemplateEditorDialog", () => {
  beforeEach(() => {
    vi.mocked(projectsApi.readPrTemplate).mockReset();
    vi.mocked(projectsApi.writePrTemplate).mockReset();
  });

  it("does not read while closed", () => {
    renderDialog({ open: false });

    expect(projectsApi.readPrTemplate).not.toHaveBeenCalled();
  });

  it("renders the shell and loading state before the read resolves", async () => {
    const read = deferred<string | null>();
    vi.mocked(projectsApi.readPrTemplate).mockReturnValue(read.promise);

    renderDialog();

    expect(
      screen.getByRole("dialog", { name: "Edit PR Template" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Loading template...")).toBeInTheDocument();
    expect(projectsApi.readPrTemplate).toHaveBeenCalledWith("project-1");

    read.resolve("# Template\n");
    await waitFor(() => {
      expect(screen.getByLabelText("Pull request template")).toHaveValue(
        "# Template\n",
      );
    });
  });

  it("hydrates exact existing content", async () => {
    vi.mocked(projectsApi.readPrTemplate).mockResolvedValue(
      "## Summary\n\nKeep trailing newline\n",
    );

    renderDialog();

    await waitFor(() => {
      expect(screen.getByLabelText("Pull request template")).toHaveValue(
        "## Summary\n\nKeep trailing newline\n",
      );
    });
  });

  it("shows the creation hint for a missing template", async () => {
    vi.mocked(projectsApi.readPrTemplate).mockResolvedValue(null);

    renderDialog();

    expect(
      await screen.findByText("Saving will create `.github/pull_request_template.md`."),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Pull request template")).toHaveValue("");
  });

  it("does not show the missing-file hint for an existing empty template", async () => {
    vi.mocked(projectsApi.readPrTemplate).mockResolvedValue("");

    renderDialog();

    await waitFor(() => {
      expect(screen.getByLabelText("Pull request template")).toHaveValue("");
    });
    expect(
      screen.queryByText("Saving will create `.github/pull_request_template.md`."),
    ).not.toBeInTheDocument();
  });

  it("saves exact draft content and closes on success", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    vi.mocked(projectsApi.readPrTemplate).mockResolvedValue("Old\n");
    vi.mocked(projectsApi.writePrTemplate).mockResolvedValue(null);

    renderDialog({ onOpenChange });
    const textarea = await screen.findByLabelText("Pull request template");
    await waitFor(() => expect(textarea).toBeEnabled());
    await user.clear(textarea);
    await user.type(textarea, "New body\nwith detail");
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(projectsApi.writePrTemplate).toHaveBeenCalledWith(
      "project-1",
      "New body\nwith detail",
    );
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
  });

  it("keeps the dialog open and preserves draft content when save fails", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    vi.mocked(projectsApi.readPrTemplate).mockResolvedValue("Old");
    vi.mocked(projectsApi.writePrTemplate).mockRejectedValue(
      new Error("write failed"),
    );

    renderDialog({ onOpenChange });
    const textarea = await screen.findByLabelText("Pull request template");
    await waitFor(() => expect(textarea).toBeEnabled());
    await user.clear(textarea);
    await user.type(textarea, "Unsaved");
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByText("write failed")).toBeInTheDocument();
    expect(screen.getByLabelText("Pull request template")).toHaveValue("Unsaved");
    expect(onOpenChange).not.toHaveBeenCalled();
  });
});
