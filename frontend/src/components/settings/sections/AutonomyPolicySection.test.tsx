import { invoke } from "@tauri-apps/api/core";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import AutonomyPolicySection from "./AutonomyPolicySection";

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
      <AutonomyPolicySection />
    </QueryClientProvider>,
  );
}

describe("AutonomyPolicySection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    let settings = {
      require_human_review: false,
      require_workspace_review: true,
      max_fix_attempts: 3,
      max_revision_cycles: 5,
      ai_review_enabled: true,
      ai_review_auto_fix: true,
      require_fix_approval: false,
      auto_create_followup_agent_conversation: false,
      autofix_workspace_review_blocking_findings: true,
      run_task_validations: true,
    };

    invokeMock.mockImplementation(async (command, args) => {
      if (command === "get_review_settings") {
        return settings;
      }
      if (command === "update_review_settings") {
        const input = (
          args as {
            input: { autoCreateFollowupAgentConversation?: boolean };
          }
        ).input;
        settings = {
          ...settings,
          auto_create_followup_agent_conversation:
            input.autoCreateFollowupAgentConversation ??
            settings.auto_create_followup_agent_conversation,
        };
        return settings;
      }
      return undefined;
    });
  });

  it("renders automatic follow-ups unchecked and lets the user opt in", async () => {
    const user = userEvent.setup();
    renderSection();

    const toggle = await screen.findByTestId(
      "auto-create-followup-agent-conversation",
    );
    expect(toggle).toHaveAttribute("aria-checked", "false");

    await user.click(toggle);

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("update_review_settings", {
        input: { autoCreateFollowupAgentConversation: true },
      }),
    );
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
  });
});
