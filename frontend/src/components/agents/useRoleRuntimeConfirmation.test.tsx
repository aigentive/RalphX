import {
  fireEvent,
  render as renderTestingLibrary,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactElement } from "react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { manualRoleDefaultsApi } from "@/api/manual-role-defaults";
import {
  harnessProvidersApi,
  type AgentProvidersSettingsResponse,
} from "@/api/harness-providers";
import { harnessProviderKeys } from "@/hooks/useHarnessProviders";
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

function render(ui: ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return renderTestingLibrary(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

function providerSettings(
  available = true,
): AgentProvidersSettingsResponse {
  return {
    defaultProvider: "claude",
    requiresOnboarding: false,
    providers: [
      {
        provider: "claude",
        enabled: true,
        isDefault: true,
        claudeDangerouslySkipPermissions: false,
        claudeAllowDangerouslySkipPermissions: false,
        available,
        binaryFound: available,
        missingCoreExecFeatures: [],
        error: available ? null : "Claude CLI is unavailable",
        status: available ? "ready" : "unavailable",
        ultraSupportedModels: [],
        supportsFastMode: false,
        fastModeSupportedModels: [],
        updatedAt: "2026-08-03T00:00:00.000Z",
      },
    ],
  };
}

function renderWithCachedProviders(ui: ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  queryClient.setQueryData(harnessProviderKeys.list(false), providerSettings());
  return renderTestingLibrary(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
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
}: {
  onReview: (runtime: typeof reviewerRuntime) => Promise<unknown>;
  onRepair: (runtime: typeof repairRuntime) => Promise<unknown>;
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
    vi.mocked(manualRoleDefaultsApi.list).mockReset();
    vi.mocked(harnessProvidersApi.list)
      .mockReset()
      .mockResolvedValue(providerSettings());
  });

  it("becomes actionable from cached providers without awaiting runtime refresh", async () => {
    let resolveRefresh!: (value: AgentProvidersSettingsResponse) => void;
    vi.mocked(manualRoleDefaultsApi.list).mockResolvedValue(
      catalog("workspace_reviewer"),
    );
    vi.mocked(harnessProvidersApi.list).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveRefresh = resolve;
        }),
    );
    const user = userEvent.setup();

    renderWithCachedProviders(
      <Harness onReview={vi.fn()} onRepair={vi.fn()} />,
    );
    await user.click(screen.getByRole("button", { name: "Open review" }));
    const dialog = await screen.findByRole("alertdialog");

    await waitFor(() =>
      expect(
        within(dialog).getByRole("button", { name: "Start review" }),
      ).toBeEnabled(),
    );
    expect(harnessProvidersApi.list).toHaveBeenCalledWith({ refreshRuntime: true });
    resolveRefresh(providerSettings());
  });

  it("disables the current confirmation when background refresh invalidates it", async () => {
    let resolveRefresh!: (value: AgentProvidersSettingsResponse) => void;
    vi.mocked(manualRoleDefaultsApi.list).mockResolvedValue(
      catalog("workspace_reviewer"),
    );
    vi.mocked(harnessProvidersApi.list).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveRefresh = resolve;
        }),
    );
    const user = userEvent.setup();

    renderWithCachedProviders(
      <Harness onReview={vi.fn()} onRepair={vi.fn()} />,
    );
    await user.click(screen.getByRole("button", { name: "Open review" }));
    const dialog = await screen.findByRole("alertdialog");
    await waitFor(() =>
      expect(
        within(dialog).getByRole("button", { name: "Start review" }),
      ).toBeEnabled(),
    );

    resolveRefresh(providerSettings(false));

    await waitFor(() =>
      expect(
        within(dialog).getByRole("button", { name: "Start review" }),
      ).toBeDisabled(),
    );
  });

  it("awaits a runtime refresh when no provider cache exists", async () => {
    let resolveRefresh!: (value: AgentProvidersSettingsResponse) => void;
    vi.mocked(manualRoleDefaultsApi.list).mockResolvedValue(
      catalog("workspace_reviewer"),
    );
    vi.mocked(harnessProvidersApi.list).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveRefresh = resolve;
        }),
    );
    const user = userEvent.setup();

    render(<Harness onReview={vi.fn()} onRepair={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: "Open review" }));
    const dialog = await screen.findByRole("alertdialog");
    expect(
      within(dialog).getByRole("button", { name: "Preparing..." }),
    ).toBeDisabled();
    await waitFor(() =>
      expect(harnessProvidersApi.list).toHaveBeenCalledWith({
        refreshRuntime: true,
      }),
    );

    resolveRefresh(providerSettings());

    await waitFor(() =>
      expect(
        within(dialog).getByRole("button", { name: "Start review" }),
      ).toBeEnabled(),
    );
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
});
