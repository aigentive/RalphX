import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { FEATURE_FLAGS_QUERY_KEY } from "@/hooks/useFeatureFlags";
import { useUiStore } from "@/stores/uiStore";
import type { FeatureFlags } from "@/types/feature-flags";

import { PersonasSection } from "./PersonasSection";

const disabledFlags: FeatureFlags = {
  activityPage: true,
  extensibilityPage: true,
  automationsPage: true,
  atlassianOauth: false,
  ticketingDashboard: false,
  agentPersonas: false,
};

vi.mock("@/hooks/usePersonas", () => ({
  usePersonas: () => ({ data: [], error: null, isLoading: false }),
  useCreatePersonaDraft: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useUpdatePersona: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useApprovePersona: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useArchivePersona: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useDeletePersonaDraft: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useUnarchivePersona: () => ({ mutateAsync: vi.fn(), isPending: false }),
  usePersonaUsage: () => ({ data: [], isLoading: false, isError: false }),
}));

function renderSection(flags: FeatureFlags) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
  queryClient.setQueryData(FEATURE_FLAGS_QUERY_KEY, flags);

  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delayDuration={0}>
        <PersonasSection />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

describe("PersonasSection toggle", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useUiStore.getState().setFeatureFlags(disabledFlags);
  });

  it("renders an unchecked toggle without persona management when disabled", () => {
    renderSection(disabledFlags);

    expect(screen.getByRole("switch", { name: "Enable agent personas" })).not.toBeChecked();
    expect(screen.queryByText("Persona library")).not.toBeInTheDocument();
  });

  it("renders a checked toggle and persona management when enabled", () => {
    renderSection({ ...disabledFlags, agentPersonas: true });

    expect(screen.getByRole("switch", { name: "Enable agent personas" })).toBeChecked();
    expect(screen.getByText("Persona library")).toBeInTheDocument();
  });

  it("enables personas through the update command and updates live consumers", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockResolvedValue({ ...disabledFlags, agentPersonas: true });
    renderSection(disabledFlags);

    await user.click(screen.getByRole("switch", { name: "Enable agent personas" }));

    expect(invoke).toHaveBeenCalledWith("update_ui_feature_flags", {
      input: { agentPersonas: true },
    });
    await waitFor(() =>
      expect(screen.getByText("Persona library")).toBeInTheDocument(),
    );
    expect(useUiStore.getState().featureFlags.agentPersonas).toBe(true);
  });

  it("disables personas through the update command", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockResolvedValue({ ...disabledFlags, agentPersonas: false });
    renderSection({ ...disabledFlags, agentPersonas: true });

    await user.click(screen.getByRole("switch", { name: "Enable agent personas" }));

    expect(invoke).toHaveBeenCalledWith("update_ui_feature_flags", {
      input: { agentPersonas: false },
    });
  });
});
