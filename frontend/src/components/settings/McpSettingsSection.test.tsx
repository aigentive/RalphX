import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { McpSettingsSection } from "./McpSettingsSection";

if (!HTMLElement.prototype.hasPointerCapture) {
  Object.defineProperty(HTMLElement.prototype, "hasPointerCapture", {
    value: () => false,
  });
}
if (!HTMLElement.prototype.setPointerCapture) {
  Object.defineProperty(HTMLElement.prototype, "setPointerCapture", {
    value: vi.fn(),
  });
}
if (!HTMLElement.prototype.releasePointerCapture) {
  Object.defineProperty(HTMLElement.prototype, "releasePointerCapture", {
    value: vi.fn(),
  });
}
if (!HTMLElement.prototype.scrollIntoView) {
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    value: vi.fn(),
  });
}

const testState = vi.hoisted(() => ({
  providers: [] as Array<Record<string, unknown>>,
  defaultProvider: null as string | null,
  catalog: undefined as Record<string, unknown> | undefined,
  openModal: vi.fn(),
  updateServer: vi.fn().mockResolvedValue(undefined),
  updateTool: vi.fn().mockResolvedValue(undefined),
  refreshProvider: vi.fn().mockResolvedValue(undefined),
  retryLegacyRepair: vi.fn().mockResolvedValue(undefined),
  hookCalls: [] as Array<[string | null, string | null, boolean]>,
  activeProject: null as { id: string; name: string } | null,
  modalContext: undefined as Record<string, unknown> | undefined,
}));

vi.mock("@/hooks/useHarnessProviders", () => ({
  useHarnessProviders: () => ({
    providers: testState.providers,
    settings: {
      providers: testState.providers,
      defaultProvider: testState.defaultProvider,
      requiresOnboarding: testState.providers.length === 0,
    },
    isLoading: false,
    isError: false,
    refetchProviders: vi.fn(),
  }),
}));

vi.mock("@/hooks/useMcpPolicy", () => ({
  useMcpPolicy: (
    projectId: string | null,
    providerName: string | null,
    enabled: boolean,
  ) => {
    testState.hookCalls.push([projectId, providerName, enabled]);
    return {
      catalog: testState.catalog,
      isLoading: false,
      isFetching: false,
      isUpdating: false,
      error: null,
      updateServer: testState.updateServer,
      updateTool: testState.updateTool,
      refreshProvider: testState.refreshProvider,
      retryLegacyRepair: testState.retryLegacyRepair,
    };
  },
}));

vi.mock("@/stores/projectStore", () => ({
  selectActiveProject: (state: { activeProject: unknown }) => state.activeProject,
  useProjectStore: (selector: (state: { activeProject: { id: string; name: string } | null }) => unknown) =>
    selector({ activeProject: testState.activeProject }),
}));

vi.mock("@/stores/uiStore", () => ({
  useUiStore: (selector: (state: {
    openModal: typeof testState.openModal;
    modalContext: Record<string, unknown> | undefined;
  }) => unknown) => selector({
    openModal: testState.openModal,
    modalContext: testState.modalContext,
  }),
}));

vi.mock("./SettingsDialog.performance", () => ({
  scheduleAfterPaint: (callback: () => void) => {
    callback();
    return { frame: null, timer: null };
  },
  cancelScheduledJob: vi.fn(),
}));

vi.mock("./ExternalMcpSettingsPanel", () => ({
  ExternalMcpSettingsPanel: () => <div>RalphX external bridge controls</div>,
}));

function provider(provider: string, enabled: boolean, available: boolean) {
  return { provider, enabled, available };
}

function server(providerName: string, serverId: string, locked = false) {
  return {
    provider: providerName,
    serverId,
    nativeScope: "user",
    nativeState: "enabled",
    effectiveEnabled: true,
    configuredState: "follow",
    effectiveState: "enabled",
    effectiveSource: locked ? "required_internal" : "provider_native",
    knownTools: [],
    disabledTools: [],
    locked,
    lockedReason: locked ? "Required by RalphX" : null,
    diagnostic: null,
    conflictKind: null,
    repairStatus: null,
  };
}

describe("McpSettingsSection", () => {
  beforeEach(() => {
    testState.providers = [];
    testState.defaultProvider = null;
    testState.catalog = undefined;
    testState.hookCalls = [];
    testState.activeProject = null;
    testState.modalContext = undefined;
    vi.clearAllMocks();
  });

  it("does not enable catalog loading until a provider is enabled and validated", async () => {
    const user = userEvent.setup();
    testState.providers = [provider("claude", true, false)];

    render(<McpSettingsSection />);

    expect(screen.getByText("No validated provider is enabled")).toBeInTheDocument();
    expect(testState.hookCalls.at(-1)).toEqual([null, null, false]);
    expect(screen.queryByText("RalphX external bridge controls")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Manage providers" }));
    expect(testState.openModal).toHaveBeenCalledWith("settings", {
      section: "providers",
    });
  });

  it("selects the eligible default provider and locks required internal servers", async () => {
    const user = userEvent.setup();
    testState.providers = [
      provider("claude", true, true),
      provider("codex", true, true),
    ];
    testState.defaultProvider = "codex";
    testState.catalog = {
      eligibleProviders: ["claude", "codex"],
      eligibleDefaultProvider: "codex",
      providerDiagnostics: {},
      policyDiagnostics: [],
      probeStale: false,
      servers: [
        server("claude", "filesystem"),
        server("codex", "github"),
        server("codex", "ralphx-internal", true),
      ],
    };

    render(<McpSettingsSection />);

    expect(screen.getByRole("combobox", { name: "MCP provider" })).toHaveTextContent("Codex");
    expect(screen.getByText("github")).toBeInTheDocument();
    expect(screen.queryByText("filesystem")).not.toBeInTheDocument();
    expect(screen.getByText("ralphx-internal")).toBeInTheDocument();
    expect(screen.queryByRole("combobox", { name: "ralphx-internal policy" })).not.toBeInTheDocument();

    const policySelect = screen.getByRole("combobox", { name: "github policy" });
    policySelect.focus();
    await user.keyboard("{Enter}{ArrowDown}{ArrowDown}{Enter}");
    expect(testState.updateServer).toHaveBeenCalledWith({
      projectId: null,
      provider: "codex",
      serverId: "github",
      state: "disabled",
    });
  });

  it("applies project-scoped exact denies and tool overrides", async () => {
    const user = userEvent.setup();
    testState.activeProject = { id: "project-1", name: "Roadmap" };
    testState.providers = [provider("claude", true, true)];
    testState.defaultProvider = "claude";
    testState.catalog = {
      eligibleProviders: ["claude"],
      eligibleDefaultProvider: "claude",
      providerDiagnostics: {
        claude: "Structured catalog is unavailable; showing limited metadata.",
      },
      policyDiagnostics: ["Project MCP policy: invalid tool identifier"],
      probeStale: true,
      servers: [
        {
          ...server("claude", "notion"),
          nativeState: "pending_approval",
          effectiveEnabled: false,
          configuredState: "disabled",
          effectiveState: "disabled",
          effectiveSource: "project_ui",
          diagnostic: "Provider approval required",
          knownTools: [
            {
              toolName: "delete_page",
              configuredState: "follow",
              effectiveState: "enabled",
              effectiveSource: "provider_native",
            },
          ],
        },
      ],
    };

    render(<McpSettingsSection />);

    await user.click(screen.getByRole("tab", { name: "Project Overrides" }));
    expect(screen.getByText(/Overrides for Roadmap/)).toBeInTheDocument();
    expect(screen.getByText("Provider approval required")).toBeInTheDocument();
    expect(
      screen.getByText("Enable or approve this server in Claude before RalphX can use it."),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Refresh Claude MCP catalog" }));
    expect(testState.refreshProvider).toHaveBeenCalledWith("claude");

    expect(screen.queryByText("Tool controls are unavailable while this server is disabled.")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Expand notion tools" }));
    expect(screen.getByText("Tool controls are unavailable while this server is disabled.")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "notion delete_page policy" })).toBeDisabled();
    expect(screen.getByText("delete_page").closest("div.max-h-64")).toHaveClass(
      "overflow-y-auto",
      "overscroll-contain",
    );
    expect(screen.getByText("Structured catalog is unavailable; showing limited metadata.")).toBeInTheDocument();
    expect(screen.getByText("Project MCP policy: invalid tool identifier")).toBeInTheDocument();
    expect(screen.getByText(/readiness result is stale/i)).toBeInTheDocument();

    await user.type(screen.getByRole("textbox", { name: "Exact MCP server ID" }), "linear");
    await user.click(screen.getByRole("button", { name: "Add deny" }));
    expect(testState.updateServer).toHaveBeenCalledWith({
      projectId: "project-1",
      provider: "claude",
      serverId: "linear",
      state: "disabled",
    });
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Exact MCP server ID" })).toHaveValue(""),
    );

    await user.type(screen.getByRole("textbox", { name: "Exact MCP server ID" }), "slack");
    await user.type(screen.getByRole("textbox", { name: "Exact MCP tool name" }), "post_message");
    await user.click(screen.getByRole("button", { name: "Add deny" }));
    expect(testState.updateTool).toHaveBeenCalledWith({
      projectId: "project-1",
      provider: "claude",
      serverId: "slack",
      toolName: "post_message",
      state: "disabled",
    });
  });

  it("uses backend catalog eligibility and default after bootstrap", async () => {
    testState.providers = [
      provider("claude", true, true),
      provider("codex", true, true),
    ];
    testState.defaultProvider = "codex";
    testState.catalog = {
      eligibleProviders: ["claude"],
      eligibleDefaultProvider: "claude",
      providerDiagnostics: {},
      policyDiagnostics: [],
      probeStale: false,
      servers: [server("claude", "filesystem"), server("codex", "github")],
    };

    render(<McpSettingsSection />);

    await waitFor(() =>
      expect(screen.getByRole("combobox", { name: "MCP provider" })).toHaveTextContent("Claude"),
    );
    expect(screen.getByText("filesystem")).toBeInTheDocument();
    expect(screen.queryByText("github")).not.toBeInTheDocument();
  });

  it("deep-links to a reserved conflict and retries cleanup without terminal guidance", async () => {
    const user = userEvent.setup();
    testState.modalContext = {
      section: "mcp",
      provider: "claude",
      serverId: "ralphx",
      scope: "user",
    };
    testState.providers = [provider("claude", true, true)];
    testState.defaultProvider = "claude";
    testState.catalog = {
      eligibleProviders: ["claude"],
      eligibleDefaultProvider: "claude",
      providerDiagnostics: {},
      policyDiagnostics: [],
      probeStale: false,
      servers: [
        {
          ...server("claude", "ralphx", true),
          effectiveEnabled: false,
          lockedReason: "Native provider configuration already defines reserved server ID 'ralphx'",
          diagnostic: "RalphX can safely remove its obsolete user registration.",
          conflictKind: "legacy_registration",
          repairStatus: "repairable",
        },
      ],
    };

    render(<McpSettingsSection />);

    expect(await screen.findByText("Reserved ID conflict")).toBeInTheDocument();
    expect(document.querySelector('[data-mcp-server-id="ralphx"]')).toHaveFocus();
    expect(screen.queryByText("claude mcp remove ralphx -s user")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Retry cleanup" }));
    expect(testState.retryLegacyRepair).toHaveBeenCalledWith({
      provider: "claude",
      serverId: "ralphx",
      scope: "user",
    });
  });
});
