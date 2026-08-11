import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { manualRoleDefaultsApi } from "./manual-role-defaults";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const rawValue = {
  provider: "codex",
  model: "gpt-5.6",
  effort: "xhigh",
  service_tier: "standard",
  coordination_mode: "solo",
  persona_id: "persona-1",
  approval_policy: "never",
  sandbox_mode: "danger-full-access",
};

describe("manualRoleDefaultsApi", () => {
  beforeEach(() => vi.clearAllMocks());

  it("parses the backend snake_case catalog into camelCase values", async () => {
    vi.mocked(invoke).mockResolvedValue({
      project_id: "project-1",
      roles: [
        {
          role: "workspace_edit",
          display_name: "Edit",
          description: "Implements changes in the selected project workspace.",
          family: "workspace",
          family_display_name: "Workspace",
          configured: rawValue,
          effective: rawValue,
          source: "project_ui",
          diagnostics: [],
          controls: {
            capabilities: [
              { value: "solo", enabled: true, disabled_reason: null },
            ],
            speeds: [
              { value: "standard", enabled: true, disabled_reason: null },
            ],
            persona: { enabled: true, disabled_reason: null },
          },
        },
      ],
    });

    await expect(manualRoleDefaultsApi.list("project-1")).resolves.toMatchObject({
      projectId: "project-1",
      roles: [
        {
          displayName: "Edit",
          description: "Implements changes in the selected project workspace.",
          familyDisplayName: "Workspace",
          configured: {
            serviceTier: "standard",
            coordinationMode: "solo",
            personaId: "persona-1",
          },
          source: "project_ui",
        },
      ],
    });
    expect(invoke).toHaveBeenCalledWith("get_manual_role_defaults", {
      projectId: "project-1",
    });
  });

  it.each([
    ["missing", undefined],
    ["empty", ""],
  ])("rejects a %s backend-owned role description", async (_label, description) => {
    vi.mocked(invoke).mockResolvedValue({
      project_id: null,
      roles: [
        {
          role: "workspace_edit",
          display_name: "Edit",
          ...(description !== undefined && { description }),
          family: "workspace",
          family_display_name: "Workspace",
          configured: null,
          effective: rawValue,
          source: "provider_default",
          diagnostics: [],
          controls: {
            capabilities: [],
            speeds: [],
            persona: { enabled: false, disabled_reason: null },
          },
        },
      ],
    });

    await expect(manualRoleDefaultsApi.list(null)).rejects.toThrow();
  });

  it("sends the exact whole role value and clears only the selected scope", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce(rawValue)
      .mockResolvedValueOnce(true);

    const value = {
      provider: "codex",
      model: "gpt-5.6",
      effort: "xhigh",
      serviceTier: "standard" as const,
      coordinationMode: "solo",
      personaId: "persona-1",
      approvalPolicy: "never",
      sandboxMode: "danger-full-access",
    };
    await manualRoleDefaultsApi.update({
      projectId: "project-1",
      role: "workspace_edit",
      value,
    });
    await manualRoleDefaultsApi.clear({
      projectId: "project-1",
      role: "workspace_edit",
    });

    expect(invoke).toHaveBeenNthCalledWith(1, "update_manual_role_default", {
      input: { projectId: "project-1", role: "workspace_edit", value },
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "clear_manual_role_default", {
      input: { projectId: "project-1", role: "workspace_edit" },
    });
  });

  it("resets an active conversation through the atomic backend command", async () => {
    vi.mocked(invoke).mockResolvedValue({
      role: "workspace_edit",
      source: "project_ui",
      value: rawValue,
    });

    await expect(
      manualRoleDefaultsApi.resetConversation({
        conversationId: "conversation-1",
      }),
    ).resolves.toMatchObject({
      role: "workspace_edit",
      source: "project_ui",
      value: {
        serviceTier: "standard",
        coordinationMode: "solo",
        personaId: "persona-1",
      },
    });
    expect(invoke).toHaveBeenCalledWith(
      "reset_agent_conversation_role_default",
      { input: { conversationId: "conversation-1" } },
    );
  });
});
