import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { manualRoleDefaultsApi } from "@/api/manual-role-defaults";
import { useAgentSessionStore } from "@/stores/agentSessionStore";

import { useRoleRuntimeConfirmation } from "./useRoleRuntimeConfirmation";

const reviewerRuntime = {
  provider: "claude",
  model: "reviewer-model",
  effort: "high",
  serviceTier: "provider_default" as const,
  coordinationMode: "solo" as const,
  personaId: null,
};
const repairRuntime = {
  ...reviewerRuntime,
  model: "repair-model",
};

function catalog(role: "workspace_reviewer" | "workspace_repair") {
  const effective = role === "workspace_reviewer" ? reviewerRuntime : repairRuntime;
  return {
    roles: [{
      role,
      displayName: role === "workspace_reviewer" ? "Reviewer" : "Repair",
      description: "Runtime test role",
      family: "workspace_review",
      familyDisplayName: "Feedback Loops",
      configured: null,
      effective,
      source: "provider_default",
      diagnostics: [],
      controls: {
        capabilities: [{ value: "solo", enabled: true, disabledReason: null }],
        speeds: [{ value: "provider_default", enabled: true, disabledReason: null }],
        persona: { enabled: false, disabledReason: "Personas are unavailable" },
      },
    }],
  };
}

vi.mock("@/api/manual-role-defaults", () => ({
  manualRoleDefaultsApi: { list: vi.fn() },
}));

vi.mock("@/api/harness-providers", () => ({
  harnessProvidersApi: {
    list: vi.fn().mockResolvedValue({
      defaultProvider: "claude",
      requiresOnboarding: false,
      providers: [{
        provider: "claude",
        enabled: true,
        available: true,
        missingCoreExecFeatures: [],
        error: null,
        status: "ready",
      }],
    }),
  },
}));

vi.mock("@/hooks/useAgentModels", () => ({
  useAgentModels: () => ({
    registry: {
      claude: [reviewerRuntime.model, repairRuntime.model].map((id) => ({
        id,
        label: id,
        menuLabel: id,
        defaultEffort: "high",
        supportedEfforts: ["high"],
      })),
    },
  }),
}));

vi.mock("@/hooks/usePersonas", () => ({
  usePersonas: () => ({ data: [] }),
}));

vi.mock("./RoleRuntimeConfirmationBody", () => ({
  RoleRuntimeConfirmationBody: ({ entry }: { entry: { role: string } }) => (
    <div>{entry.role}</div>
  ),
}));

function Harness({
  onReview,
  onRepair,
  optIn = false,
}: {
  onReview: (runtime: typeof reviewerRuntime) => Promise<unknown>;
  onRepair: (runtime: typeof repairRuntime) => Promise<unknown>;
  optIn?: boolean;
}) {
  const { confirmRoleRuntime, confirmationDialogProps, ConfirmationDialog } =
    useRoleRuntimeConfirmation({
      conversationId: "conversation-1",
      projectId: "project-1",
    });
  return (
    <>
      <button
        type="button"
        onClick={() => void confirmRoleRuntime({
          role: "workspace_reviewer",
          title: "Review",
          description: "Review runtime",
          confirmText: "Start review",
          ...(optIn && {
            optIn: {
              title: "Keep automatic review enabled",
              description: "Continue the review loop after this run.",
              initialValue: true,
            },
          }),
          onConfirm: onReview,
        })}
      >
        Open review
      </button>
      <button
        type="button"
        data-testid="open-repair"
        onClick={() => void confirmRoleRuntime({
          role: "workspace_repair",
          title: "Repair",
          description: "Repair runtime",
          confirmText: "Start repair",
          onConfirm: onRepair,
        })}
      >
        Open repair
      </button>
      <ConfirmationDialog {...confirmationDialogProps} />
    </>
  );
}

describe("useRoleRuntimeConfirmation", () => {
  beforeEach(() => {
    useAgentSessionStore.setState({ roleRuntimeOverridesByConversationId: {} });
  });

  it("does not let superseded preparation overwrite the current runtime tuple", async () => {
    let resolveFirst!: (value: ReturnType<typeof catalog>) => void;
    vi.mocked(manualRoleDefaultsApi.list)
      .mockImplementationOnce(
        () => new Promise((resolve) => { resolveFirst = resolve; }),
      )
      .mockResolvedValueOnce(catalog("workspace_repair"));
    const onReview = vi.fn().mockResolvedValue(undefined);
    const onRepair = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();

    render(<Harness onReview={onReview} onRepair={onRepair} />);
    await user.click(screen.getByRole("button", { name: "Open review" }));
    await waitFor(() => expect(manualRoleDefaultsApi.list).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByTestId("open-repair"));

    const dialog = await screen.findByRole("alertdialog");
    await waitFor(() => {
      expect(within(dialog).getByText("workspace_repair")).toBeInTheDocument();
      expect(within(dialog).getByRole("button", { name: "Start repair" })).toBeEnabled();
    });
    resolveFirst(catalog("workspace_reviewer"));
    await Promise.resolve();
    await user.click(within(dialog).getByRole("button", { name: "Start repair" }));

    await waitFor(() => expect(onRepair).toHaveBeenCalledWith(repairRuntime));
    expect(onReview).not.toHaveBeenCalled();
  });

  it("passes the changed opt-in choice with the selected runtime", async () => {
    vi.mocked(manualRoleDefaultsApi.list).mockResolvedValue(
      catalog("workspace_reviewer"),
    );
    const onReview = vi.fn().mockResolvedValue(undefined);
    const onRepair = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();

    render(<Harness onReview={onReview} onRepair={onRepair} optIn />);
    await user.click(screen.getByRole("button", { name: "Open review" }));
    const dialog = await screen.findByRole("alertdialog");
    const optInSwitch = await within(dialog).findByRole("switch", {
      name: "Keep automatic review enabled",
    });
    expect(optInSwitch).toBeChecked();
    await user.click(optInSwitch);
    await user.click(within(dialog).getByRole("button", { name: "Start review" }));

    await waitFor(() =>
      expect(onReview).toHaveBeenCalledWith(reviewerRuntime, false),
    );
  });
});
