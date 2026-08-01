import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { remoteEnvironmentsApi } from "@/api/remote-environments";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  LOCAL_ENVIRONMENT_ID,
  type EnvironmentConnectionState,
  useEnvironmentStore,
} from "@/stores/environmentStore";
import { useUiStore } from "@/stores/uiStore";
import type { RemoteEnvironmentSummary } from "@/api/remote-environments";

import { EnvironmentSwitcher } from "./EnvironmentSwitcher";
import { ENVIRONMENT_STATUS_DOT } from "./environment-switcher-status";

vi.mock("sonner", () => ({
  toast: { error: vi.fn() },
}));

vi.mock("@/api/remote-environments", () => ({
  remoteEnvironmentsApi: {
    setActiveEnvironment: vi.fn(),
  },
}));

const ALL_STATES: EnvironmentConnectionState[] = [
  "idle",
  "connecting",
  "connected",
  "backoff",
  "offline",
  "blocked",
  "suspended",
  "health_only",
];

function remote(id: string, name: string): RemoteEnvironmentSummary {
  return {
    id,
    environmentId: `host-${id}`,
    name,
    baseUrl: `https://${id}.test`,
    candidateUrls: [],
    scopes: ["ui:read", "ui:operate"],
    protocolVersion: 1,
    status: "active",
    createdAt: "2026-07-28T00:00:00Z",
    lastConnectedAt: null,
  };
}

function seed(states: EnvironmentConnectionState[] = ["connected"]): void {
  const summaries = states.map((_, index) => remote(`env-${index}`, `Remote ${index}`));
  useEnvironmentStore.getState().setEnvironments(summaries);
  states.forEach((state, index) => {
    useEnvironmentStore.getState().setConnectionState(`env-${index}`, state);
  });
}

function renderSwitcher(props: Partial<Parameters<typeof EnvironmentSwitcher>[0]> = {}) {
  return render(
    <TooltipProvider delayDuration={0}>
      <EnvironmentSwitcher {...props} />
    </TooltipProvider>,
  );
}

async function openSwitcher(): Promise<void> {
  // Prefix match: the accessible name grows a background-notification clause when a
  // badge is present (PR 3.3-a), so an exact match would only find the empty case.
  await userEvent.click(screen.getByRole("button", { name: /^Switch environment/ }));
}

describe("EnvironmentSwitcher", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(remoteEnvironmentsApi.setActiveEnvironment).mockResolvedValue(null);
    useUiStore.setState({
      featureFlags: {
        ...useUiStore.getState().featureFlags,
        remoteEnvironments: true,
      },
    });
    useEnvironmentStore.setState({
      activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
      environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
      connectionStates: { [LOCAL_ENVIRONMENT_ID]: "connected" },
    });
  });

  it("renders null when the flag is off or the registry contains only local", () => {
    const { rerender } = renderSwitcher();
    expect(screen.queryByRole("button", { name: "Switch environment" })).not.toBeInTheDocument();

    seed();
    useUiStore.setState({
      featureFlags: {
        ...useUiStore.getState().featureFlags,
        remoteEnvironments: false,
      },
    });
    rerender(
      <TooltipProvider delayDuration={0}>
        <EnvironmentSwitcher />
      </TooltipProvider>,
    );
    expect(screen.queryByRole("button", { name: "Switch environment" })).not.toBeInTheDocument();
  });

  it("exports one typed dot description for every presented state and local", async () => {
    seed(ALL_STATES);
    renderSwitcher();
    await openSwitcher();

    expect(Object.keys(ENVIRONMENT_STATUS_DOT)).toEqual(ALL_STATES);
    expect(screen.getByTestId("environment-option-local").querySelector("[data-status]")).toHaveAttribute(
      "data-status",
      "connected",
    );
    ALL_STATES.forEach((state, index) => {
      const dot = screen
        .getByTestId(`environment-option-env-${index}`)
        .querySelector("[data-status]");
      expect(dot).toHaveAttribute("data-status", state);
      expect(dot).toHaveTextContent(ENVIRONMENT_STATUS_DOT[state].glyph);
    });
  });

  it("marks the active row and exposes trigger and status tooltips", async () => {
    seed(["backoff", "blocked"]);
    renderSwitcher();

    const trigger = screen.getByRole("button", { name: "Switch environment" });
    expect(trigger).toHaveTextContent("This Mac");
    await userEvent.hover(trigger);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Switch environment");
    await userEvent.unhover(trigger);

    await openSwitcher();
    expect(screen.getByRole("option", { name: /This Mac/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(
      screen.getByRole("option", { name: /This Mac/ }).querySelector(
        '[aria-label="Active environment"]',
      ),
    ).toBeInTheDocument();

    const reconnecting = screen.getByRole("option", { name: /Remote 0/ });
    await userEvent.hover(reconnecting);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Reconnecting…");
  });

  it("updates the shell and closes synchronously before the switch promise settles", async () => {
    seed(["connecting"]);
    let resolveSwitch: (() => void) | undefined;
    let deferredWorkStarted = false;
    vi.mocked(remoteEnvironmentsApi.setActiveEnvironment).mockImplementation(
      () => {
        queueMicrotask(() => {
          deferredWorkStarted = true;
        });
        return new Promise<null>((resolve) => {
          resolveSwitch = () => resolve(null);
        });
      },
    );
    renderSwitcher();
    await openSwitcher();

    fireEvent.click(screen.getByRole("option", { name: /Remote 0/ }));

    expect(screen.getByRole("button", { name: "Switch environment" })).toHaveTextContent(
      "Remote 0",
    );
    expect(screen.queryByRole("listbox", { name: "Environments" })).not.toBeInTheDocument();
    expect(useEnvironmentStore.getState().activeEnvironmentId).toBe("env-0");
    expect(remoteEnvironmentsApi.setActiveEnvironment).toHaveBeenCalledWith("env-0");
    expect(deferredWorkStarted).toBe(false);

    await act(async () => {
      resolveSwitch?.();
    });
    expect(deferredWorkStarted).toBe(true);
  });

  it("clicking the active row only closes the popover", async () => {
    seed();
    renderSwitcher();
    await openSwitcher();

    await userEvent.click(screen.getByRole("option", { name: /This Mac/ }));

    expect(screen.queryByRole("listbox", { name: "Environments" })).not.toBeInTheDocument();
    expect(remoteEnvironmentsApi.setActiveEnvironment).not.toHaveBeenCalled();
  });

  it("supports listbox navigation, selection, escape, and trigger focus return", async () => {
    seed(["connected", "offline"]);
    renderSwitcher();
    const trigger = screen.getByRole("button", { name: "Switch environment" });

    trigger.focus();
    await userEvent.keyboard("{ArrowDown}");
    await waitFor(() => {
      expect(screen.getByRole("option", { name: /This Mac/ })).toHaveFocus();
    });
    await userEvent.keyboard("{End}");
    expect(screen.getByRole("option", { name: /Remote 1/ })).toHaveFocus();
    await userEvent.keyboard("{Home}");
    expect(screen.getByRole("option", { name: /This Mac/ })).toHaveFocus();
    await userEvent.keyboard("{ArrowDown}{Enter}");
    expect(useEnvironmentStore.getState().activeEnvironmentId).toBe("env-0");
    expect(trigger).toHaveFocus();

    await userEvent.keyboard("{ArrowDown}{Escape}");
    expect(screen.queryByRole("listbox", { name: "Environments" })).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it("follows the store and surfaces the error when a switch is refused", async () => {
    seed(["connected"]);
    vi.mocked(remoteEnvironmentsApi.setActiveEnvironment).mockRejectedValue(
      "REMOTE_FORBIDDEN: the proxy still points at another environment",
    );
    renderSwitcher();
    await openSwitcher();

    await act(async () => {
      fireEvent.click(screen.getByRole("option", { name: /Remote 0/ }));
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Switch environment" })).toHaveTextContent(
        "This Mac",
      );
    });
    // A silent revert would leave the user with an unexplained remount flicker.
    expect(toast.error).toHaveBeenCalledWith("Could not switch to Remote 0", {
      description: "REMOTE_FORBIDDEN",
    });
  });
  describe("background notification badges (PR 3.3-a)", () => {
    it("shows no badge when nothing has been observed", async () => {
      seed(["connected", "health_only"]);
      renderSwitcher();

      expect(screen.queryByTestId("environment-switcher-badge")).not.toBeInTheDocument();
      await openSwitcher();
      expect(screen.queryByTestId("environment-badge-env-1")).not.toBeInTheDocument();
    });

    it("shows a per-environment count in the list", async () => {
      seed(["connected", "health_only"]);
      act(() => {
        useEnvironmentStore.setState({ notificationBadges: { "env-1": 3 } });
      });
      renderSwitcher();

      await openSwitcher();
      expect(screen.getByTestId("environment-badge-env-1")).toHaveTextContent("3");
      expect(screen.queryByTestId("environment-badge-env-0")).not.toBeInTheDocument();
    });

    it("caps the glyph at 9+ but keeps the exact count in the accessible name", async () => {
      seed(["connected", "health_only"]);
      act(() => {
        useEnvironmentStore.setState({ notificationBadges: { "env-1": 42 } });
      });
      renderSwitcher();

      await openSwitcher();
      const badge = screen.getByTestId("environment-badge-env-1");
      expect(badge).toHaveTextContent("9+");
      // A screen reader must hear the real number, not the truncation glyph.
      expect(badge).toHaveAttribute("aria-label", "42 new notifications");
    });

    it("sums background environments on the collapsed trigger and names the total", () => {
      seed(["connected", "health_only", "health_only"]);
      act(() => {
        useEnvironmentStore.setState({
          notificationBadges: { "env-1": 2, "env-2": 3 },
        });
      });
      renderSwitcher();

      expect(screen.getByTestId("environment-switcher-badge")).toHaveTextContent("5");
      expect(
        screen.getByRole("button", {
          name: "Switch environment, 5 new notifications in other environments",
        }),
      ).toBeInTheDocument();
    });

    it("excludes the active environment from the trigger total", () => {
      seed(["connected", "health_only"]);
      act(() => {
        useEnvironmentStore.setState({
          activeEnvironmentId: "env-0",
          notificationBadges: { "env-0": 4, "env-1": 1 },
        });
      });
      renderSwitcher();

      // The active environment is projecting: its notifications already reached its
      // own cache and its own bell, so counting them here would double-report them.
      expect(screen.getByTestId("environment-switcher-badge")).toHaveTextContent("1");
    });
  });

  describe("syncing chip", () => {
    function seedSyncing(activeId = "env-0"): void {
      seed(["connecting", "connected"]);
      act(() => {
        useEnvironmentStore.setState({ activeEnvironmentId: activeId });
        useEnvironmentStore.getState().setConnectionPresentation("env-0", {
          presentation: "syncing",
          blockedFailure: null,
          blockedMessage: null,
        });
      });
    }

    it("shows the pulsing accent dot, the label, and the syncing tooltip", async () => {
      seedSyncing();
      renderSwitcher();

      expect(
        screen.getByTestId("environment-switcher-syncing-label")
      ).toHaveTextContent("Syncing…");
      const dot = screen.getByTestId("environment-dot-env-0");
      expect(dot).toHaveAttribute("data-status", "syncing");
      expect(dot.className).toContain("remote-syncing-dot");
      const trigger = screen.getByRole("button", { name: /syncing with "Remote 0"/ });
      await userEvent.hover(trigger);
      expect(await screen.findByRole("tooltip")).toHaveTextContent(
        /Syncing with the host — read-only until it finishes/
      );
    });

    it("shows no label or syncing dot when connected", () => {
      seed(["connected"]);
      act(() => {
        useEnvironmentStore.setState({ activeEnvironmentId: "env-0" });
        useEnvironmentStore.getState().clearConnectionPresentation("env-0");
      });
      renderSwitcher();

      expect(
        screen.queryByTestId("environment-switcher-syncing-label")
      ).toBeNull();
      expect(screen.getByTestId("environment-dot-env-0")).toHaveAttribute(
        "data-status",
        "connected"
      );
    });

    it("never puts the syncing dot on a background environment", async () => {
      // env-1 is active; env-0 (background) reports syncing — its row must keep the
      // state-keyed dot, because a background environment has no stream to sync.
      seedSyncing("env-1");
      renderSwitcher();
      await openSwitcher();

      const backgroundDot = screen
        .getByRole("option", { name: /Remote 0/ })
        .querySelector('[data-testid="environment-dot-env-0"]');
      expect(backgroundDot).toHaveAttribute("data-status", "connecting");
    });

    it("opens the connection log from the dropdown footer", async () => {
      seedSyncing();
      renderSwitcher();
      await openSwitcher();

      await userEvent.click(
        screen.getByTestId("environment-switcher-connection-log")
      );

      expect(
        screen.getByTestId("remote-connection-journal-dialog")
      ).toHaveTextContent("Connection log — Remote 0");
    });
  });
});
