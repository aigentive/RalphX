import { invoke } from "@tauri-apps/api/core";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import ReviewPolicySection from "./ReviewPolicySection";

const invokeMock = vi.mocked(invoke);

function renderSection() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });

  return render(
    <QueryClientProvider client={queryClient}>
      <ReviewPolicySection />
    </QueryClientProvider>,
  );
}

describe("ReviewPolicySection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    invokeMock.mockImplementation(async (command, args) => {
      if (command === "get_review_settings") {
        return {
          require_human_review: false,
          require_workspace_review: true,
          max_fix_attempts: 3,
          max_revision_cycles: 5,
          ai_review_enabled: true,
          ai_review_auto_fix: true,
          require_fix_approval: false,
          auto_create_followup_agent_conversation: true,
          autofix_workspace_review_blocking_findings: true,
          run_task_validations: true,
        };
      }
      if (command === "update_review_settings") {
        const input = (
          args as {
            input: {
              requireHumanReview?: boolean;
              runTaskValidations?: boolean;
            };
          }
        ).input;
        return {
          require_human_review: input.requireHumanReview ?? false,
          require_workspace_review: true,
          max_fix_attempts: 3,
          max_revision_cycles: 5,
          ai_review_enabled: true,
          ai_review_auto_fix: true,
          require_fix_approval: false,
          auto_create_followup_agent_conversation: true,
          autofix_workspace_review_blocking_findings: true,
          run_task_validations: input.runTaskValidations ?? true,
        };
      }
      return undefined;
    });
  });

  it("updates the human review setting", async () => {
    const user = userEvent.setup();
    renderSection();

    await user.click(await screen.findByTestId("require-human-review"));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("update_review_settings", {
        input: { requireHumanReview: true },
      }),
    );
  });

  it("updates the task validation runner setting", async () => {
    const user = userEvent.setup();
    renderSection();

    await user.click(await screen.findByTestId("run-task-validations"));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("update_review_settings", {
        input: { runTaskValidations: false },
      }),
    );
  });
});
