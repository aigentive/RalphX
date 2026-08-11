import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { CapabilitiesSection } from "./CapabilitiesSection";

function renderSection() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <CapabilitiesSection />
    </QueryClientProvider>,
  );
}

describe("CapabilitiesSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "get_ui_feature_flags") {
        return {
          activityPage: true,
          extensibilityPage: true,
          agentConversationTeam: false,
          agentConversationWorkflows: false,
        };
      }
      if (command === "update_ui_feature_flags") {
        const input = (args as { input: Record<string, boolean> }).input;
        return {
          activityPage: true,
          extensibilityPage: true,
          agentConversationTeam: input.agentConversationTeam ?? false,
          agentConversationWorkflows: input.agentConversationWorkflows ?? false,
        };
      }
      throw new Error(`Unexpected command: ${command}`);
    });
  });

  it("does not expose always-on folder context as a capability", async () => {
    renderSection();

    expect(await screen.findByText("Team")).toBeInTheDocument();
    expect(screen.queryByText("Folder context")).not.toBeInTheDocument();
    expect(screen.getByText("Workflows")).toBeInTheDocument();
    expect(screen.getByTestId("agent-conversation-team")).not.toBeChecked();
    expect(
      screen.getByTestId("agent-conversation-workflows"),
    ).not.toBeChecked();
    expect(screen.getAllByText(/Experimental/)).toHaveLength(3);
    expect(
      screen.getByText(/Codex Ultra is availability-driven/i),
    ).toBeInTheDocument();
  });

  it("updates Team independently", async () => {
    const user = userEvent.setup();
    renderSection();

    await screen.findByText("Team");
    await user.click(screen.getByTestId("agent-conversation-team"));

    expect(invoke).toHaveBeenCalledWith("update_ui_feature_flags", {
      input: { agentConversationTeam: true },
    });
  });
});
