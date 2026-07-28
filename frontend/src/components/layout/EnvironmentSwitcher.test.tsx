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
  await userEvent.click(screen.getByRole("button", { name: "Switch environment" }));
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
});
