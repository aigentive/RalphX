import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AgentIssueReportDialog } from "./AgentIssueReportDialog";
import type { AgentIssueReportDraft } from "@/api/agent-issue-report";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";

const mocks = vi.hoisted(() => ({
  build: vi.fn(),
  submit: vi.fn(),
  save: vi.fn(),
  writeTextFile: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: mocks.save,
}));

vi.mock("@tauri-apps/plugin-fs", () => ({
  writeTextFile: mocks.writeTextFile,
}));

vi.mock("@/api/agent-issue-report", () => ({
  agentIssueReportApi: {
    build: mocks.build,
    submit: mocks.submit,
  },
}));

vi.mock("sonner", () => ({
  toast: {
    success: mocks.toastSuccess,
    error: mocks.toastError,
  },
}));

const draft: AgentIssueReportDraft = {
  conversationId: "conversation-12345678",
  projectId: "project-1",
  generatedAt: "2026-06-19T12:00:00Z",
  markdown: "# Report\n\nOriginal body",
  destination: {
    repository: "aigentive/ralphx.app",
    source: "public_default",
    isDefault: true,
  },
  redactionSummary: {
    replacements: [{ category: "home_path", count: 1 }],
  },
  sources: [
    {
      label: "stream-debug/conversation.log",
      included: true,
      truncated: false,
      detail: null,
    },
  ],
  warnings: [],
};

describe("AgentIssueReportDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.build.mockResolvedValue(draft);
    mocks.submit.mockResolvedValue({
      repository: "aigentive/ralphx.app",
      issueUrl: "https://github.com/aigentive/ralphx.app/issues/42",
    });
    mocks.save.mockResolvedValue("/tmp/report.md");
    mocks.writeTextFile.mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: vi.fn().mockResolvedValue(undefined),
      },
    });
    Object.defineProperty(window, "requestAnimationFrame", {
      configurable: true,
      value: vi.fn((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      }),
    });
    Object.defineProperty(window, "cancelAnimationFrame", {
      configurable: true,
      value: vi.fn(),
    });
    useEnvironmentStore.setState({
      activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
      environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
    });
  });

  it("renders a host-only notice remotely without building a report", () => {
    useEnvironmentStore.setState({
      activeEnvironmentId: "remote-1",
      environments: [{ id: "remote-1", name: "Studio", kind: "remote" }],
    });
    render(<AgentIssueReportDialog open onOpenChange={vi.fn()} context={{ projectId: "project-1", conversationId: "conversation-12345678" }} />);
    expect(screen.getByTestId("remote-host-only-notice")).toHaveTextContent("Studio");
    expect(mocks.build).not.toHaveBeenCalled();
    expect(screen.queryByTestId("agent-issue-report-submit")).not.toBeInTheDocument();
  });

  it("submits the edited markdown only after explicit confirmation", async () => {
    render(
      <AgentIssueReportDialog
        open
        onOpenChange={vi.fn()}
        context={{
          projectId: "project-1",
          conversationId: "conversation-12345678",
        }}
      />,
    );

    expect(screen.getByTestId("agent-issue-report-loading")).toBeInTheDocument();

    const editor = await screen.findByTestId("agent-issue-report-editor");
    expect(screen.getAllByText("aigentive/ralphx.app").length).toBeGreaterThan(0);

    fireEvent.change(editor, { target: { value: "# Edited\n\nReviewed body" } });
    fireEvent.click(screen.getByRole("button", { name: "Create GitHub Issue" }));

    expect(screen.getByTestId("agent-issue-report-confirm")).toHaveTextContent(
      "aigentive/ralphx.app",
    );
    expect(mocks.submit).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Confirm and Create" }));

    await waitFor(() => {
      expect(mocks.submit).toHaveBeenCalledWith({
        conversationId: "conversation-12345678",
        repository: "aigentive/ralphx.app",
        title: "RalphX issue report: conversa",
        bodyMarkdown: "# Edited\n\nReviewed body",
      });
    });
    expect(
      screen.getByText("https://github.com/aigentive/ralphx.app/issues/42"),
    ).toBeInTheDocument();
  });

  it("renders the empty state when no agent conversation is selected", () => {
    render(<AgentIssueReportDialog open onOpenChange={vi.fn()} context={null} />);

    expect(
      screen.getByText("Select an agent conversation to report an issue."),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Export" })).toBeDisabled();
    expect(screen.getByTestId("agent-issue-report-submit")).toBeDisabled();
  });

  it("shows build errors without enabling submission", async () => {
    mocks.build.mockRejectedValueOnce(new Error("logs unavailable"));

    render(
      <AgentIssueReportDialog
        open
        onOpenChange={vi.fn()}
        context={{
          projectId: "project-1",
          conversationId: "conversation-12345678",
        }}
      />,
    );

    expect(await screen.findByRole("alert")).toHaveTextContent("logs unavailable");
    expect(screen.getByTestId("agent-issue-report-submit")).toBeDisabled();
  });

  it("copies and exports the reviewed markdown", async () => {
    render(
      <AgentIssueReportDialog
        open
        onOpenChange={vi.fn()}
        context={{
          projectId: "project-1",
          conversationId: "conversation-12345678",
        }}
      />,
    );

    const editor = await screen.findByTestId("agent-issue-report-editor");
    fireEvent.change(editor, { target: { value: "# Reviewed\n\nExport me" } });

    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    await waitFor(() => {
      expect(navigator.clipboard.writeText).toHaveBeenCalledWith("# Reviewed\n\nExport me");
    });
    expect(mocks.toastSuccess).toHaveBeenCalledWith("Issue report copied");

    fireEvent.click(screen.getByRole("button", { name: "Export" }));
    await waitFor(() => {
      expect(mocks.save).toHaveBeenCalledWith({
        filters: [{ name: "Markdown", extensions: ["md"] }],
        defaultPath: "ralphx-issue-report-conversa.md",
      });
      expect(mocks.writeTextFile).toHaveBeenCalledWith("/tmp/report.md", "# Reviewed\n\nExport me");
    });
    expect(mocks.toastSuccess).toHaveBeenCalledWith("Issue report exported");
  });

  it("reports copy and export failures", async () => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: vi.fn().mockRejectedValue(new Error("denied")),
      },
    });
    mocks.save.mockRejectedValueOnce(new Error("dialog failed"));

    render(
      <AgentIssueReportDialog
        open
        onOpenChange={vi.fn()}
        context={{
          projectId: "project-1",
          conversationId: "conversation-12345678",
        }}
      />,
    );

    await screen.findByTestId("agent-issue-report-editor");
    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    await waitFor(() => {
      expect(mocks.toastError).toHaveBeenCalledWith("Failed to copy issue report");
    });

    fireEvent.click(screen.getByRole("button", { name: "Export" }));
    await waitFor(() => {
      expect(mocks.toastError).toHaveBeenCalledWith("Failed to export issue report");
    });
  });

  it("renders the markdown preview and clears confirmation after editing", async () => {
    render(
      <AgentIssueReportDialog
        open
        onOpenChange={vi.fn()}
        context={{
          projectId: "project-1",
          conversationId: "conversation-12345678",
        }}
      />,
    );

    const editor = await screen.findByTestId("agent-issue-report-editor");
    fireEvent.click(screen.getByRole("button", { name: "Create GitHub Issue" }));
    expect(screen.getByTestId("agent-issue-report-confirm")).toBeInTheDocument();

    fireEvent.change(editor, { target: { value: "# Previewed\n\nUpdated body" } });
    expect(screen.queryByTestId("agent-issue-report-confirm")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agent-issue-report-preview-tab"));
    expect(await screen.findByText("Previewed")).toBeInTheDocument();
  });
});
