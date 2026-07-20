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
          composerFolderReferences: false,
          agentConversationTeam: false,
          agentConversationWorkflows: false,
        };
      }
      if (command === "update_ui_feature_flags") {
        const input = (args as { input: Record<string, boolean> }).input;
        return {
          activityPage: true,
          extensibilityPage: true,
          composerFolderReferences:
            input.composerFolderReferences ?? false,
          agentConversationTeam: input.agentConversationTeam ?? false,
          agentConversationWorkflows: input.agentConversationWorkflows ?? false,
        };
      }
      throw new Error(`Unexpected command: ${command}`);
    });
  });

  it("renders Folder context, Team, and Workflows off by default", async () => {
    renderSection();

    expect(await screen.findByText("Folder context")).toBeInTheDocument();
    expect(await screen.findByText("Team")).toBeInTheDocument();
    expect(screen.getByText("Workflows")).toBeInTheDocument();
    expect(screen.getByTestId("composer-folder-references")).not.toBeChecked();
    expect(screen.getByTestId("agent-conversation-team")).not.toBeChecked();
    expect(
      screen.getByTestId("agent-conversation-workflows"),
    ).not.toBeChecked();
    expect(screen.getAllByText(/Experimental/)).toHaveLength(2);
    expect(
      screen.getByText(/Codex Ultra is availability-driven/i),
    ).toBeInTheDocument();
  });

  it("updates Folder context independently", async () => {
    const user = userEvent.setup();
    renderSection();

    await user.click(
      await screen.findByTestId("composer-folder-references"),
    );

    expect(invoke).toHaveBeenCalledWith("update_ui_feature_flags", {
      input: { composerFolderReferences: true },
    });
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
