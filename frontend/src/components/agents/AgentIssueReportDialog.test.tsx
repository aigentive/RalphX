import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AgentIssueReportDialog } from "./AgentIssueReportDialog";
import type { AgentIssueReportDraft } from "@/api/agent-issue-report";

const mocks = vi.hoisted(() => ({
  build: vi.fn(),
  submit: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
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
});
