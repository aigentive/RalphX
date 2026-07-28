/**
 * PR 2.7-a presentation contract.
 *
 * Fake timers throughout: the load-bearing half of this surface is what it does NOT do.
 * A-5 makes the supervisor the sole retry owner, so the banner must never schedule a
 * timer — not while reconnecting, and least of all while blocked, where a timer would
 * redial a host that already refused this device.
 */

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import type { AttemptFailure } from "@/lib/remote/supervisor";
import type { SupervisorPresentation } from "@/lib/remote/supervisor-transition-table";
import {
  LOCAL_ENVIRONMENT_ID,
  useEnvironmentStore,
} from "@/stores/environmentStore";
import { useUiStore } from "@/stores/uiStore";

const { retryActiveEnvironmentNow } = vi.hoisted(() => ({
  retryActiveEnvironmentNow: vi.fn(),
}));

vi.mock("@/lib/remote/environment-runtime", () => ({
  retryActiveEnvironmentNow,
}));

import { RemoteConnectionBanner } from "./RemoteConnectionBanner";

const REMOTE_ID = "env-studio";
const REMOTE_NAME = "Studio Mac";

function seed({
  presentation,
  blockedFailure = null,
  blockedMessage = null,
  active = REMOTE_ID,
  enabled = true,
}: {
  presentation?: SupervisorPresentation;
  blockedFailure?: AttemptFailure | null;
  blockedMessage?: string | null;
  active?: string;
  enabled?: boolean;
}): void {
  useUiStore.setState((state) => ({
    featureFlags: { ...state.featureFlags, remoteEnvironments: enabled },
  }));
  useEnvironmentStore.setState({
    activeEnvironmentId: active,
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      { id: REMOTE_ID, name: REMOTE_NAME, kind: "remote" },
    ],
    connectionPresentations:
      presentation === undefined
        ? {}
        : {
            [REMOTE_ID]: { presentation, blockedFailure, blockedMessage },
          },
  });
}

function renderBanner() {
  return render(
    <TooltipProvider>
      <RemoteConnectionBanner />
    </TooltipProvider>
  );
}

beforeEach(() => {
  vi.useFakeTimers();
  retryActiveEnvironmentNow.mockClear();
});

afterEach(() => {
  cleanup();
  expect(
    vi.getTimerCount(),
    "the banner scheduled a timer; A-5 makes the supervisor the sole retry owner"
  ).toBe(0);
  vi.useRealTimers();
  useEnvironmentStore.setState({
    activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
    connectionPresentations: {},
  });
});

describe("presentation variants", () => {
  it("names the first-ever connect as connecting, with no read-only claim", () => {
    seed({ presentation: "connecting" });
    renderBanner();

    const banner = screen.getByTestId("remote-connection-banner");
    expect(banner).toHaveTextContent(`Connecting to "${REMOTE_NAME}"…`);
    expect(banner).not.toHaveTextContent(/read-only/i);
  });

  it("names a later attempt as reconnecting, with the read-only claim", () => {
    seed({ presentation: "reconnecting" });
    renderBanner();

    const banner = screen.getByTestId("remote-connection-banner");
    expect(banner).toHaveTextContent(`Reconnecting to "${REMOTE_NAME}"…`);
    expect(banner).toHaveTextContent(/read-only until the connection returns/i);
  });

  it("explains offline as a network condition, not a host failure", () => {
    seed({ presentation: "offline" });
    renderBanner();

    const banner = screen.getByTestId("remote-connection-banner");
    expect(banner).toHaveTextContent("You're offline");
    expect(banner).toHaveTextContent(
      `"${REMOTE_NAME}" will reconnect when the network returns`
    );
  });

  it.each<[SupervisorPresentation | undefined, string]>([
    ["connected", "a healthy connection"],
    ["suspended", "a backgrounded app"],
    [undefined, "an environment no supervisor has reported on"],
  ])("renders nothing for %s (%s)", (presentation) => {
    seed(presentation === undefined ? {} : { presentation });
    renderBanner();

    expect(screen.queryByTestId("remote-connection-banner")).toBeNull();
  });

  it("renders nothing for the local environment", () => {
    seed({ presentation: "reconnecting", active: LOCAL_ENVIRONMENT_ID });
    renderBanner();

    expect(screen.queryByTestId("remote-connection-banner")).toBeNull();
  });

  it("renders nothing when the remoteEnvironments flag is off (dark ship)", () => {
    seed({ presentation: "offline", enabled: false });
    renderBanner();

    expect(screen.queryByTestId("remote-connection-banner")).toBeNull();
  });
});

describe("blocked triple", () => {
  it("offers an update path for a version block", () => {
    seed({
      presentation: "error",
      blockedFailure: "version",
      blockedMessage: "host requires >= v2, this app speaks v1",
    });
    renderBanner();

    const banner = screen.getByTestId("remote-connection-banner");
    expect(banner).toHaveTextContent(`"${REMOTE_NAME}" needs a newer client`);
    expect(banner).toHaveTextContent("host requires >= v2, this app speaks v1");
    expect(screen.getByTestId("remote-connection-banner-retry")).toBeInTheDocument();
    expect(screen.queryByTestId("remote-connection-banner-repair")).toBeNull();
  });

  it("offers re-pairing — not a retry — for a revoked device", () => {
    seed({
      presentation: "error",
      blockedFailure: "unauthorized",
      blockedMessage: "The host ended this device's session (revoked).",
    });
    renderBanner();

    expect(screen.getByTestId("remote-connection-banner")).toHaveTextContent(
      `Access to "${REMOTE_NAME}" was revoked`
    );
    expect(screen.getByTestId("remote-connection-banner-repair")).toBeInTheDocument();
    // Retrying a revoked credential cannot succeed, so the affordance is absent.
    expect(screen.queryByTestId("remote-connection-banner-retry")).toBeNull();
  });

  it("names a malformed descriptor as a host identity problem", () => {
    seed({
      presentation: "error",
      blockedFailure: "malformed_descriptor",
      blockedMessage: "descriptor environmentId did not match the paired host",
    });
    renderBanner();

    expect(screen.getByTestId("remote-connection-banner")).toHaveTextContent(
      `"${REMOTE_NAME}" sent an invalid identity response`
    );
  });

  it("stays blocked-shaped for an unrecognised cause (fail closed)", () => {
    seed({ presentation: "error", blockedFailure: null });
    renderBanner();

    expect(screen.getByTestId("remote-connection-banner")).toHaveTextContent(
      `"${REMOTE_NAME}" can't be reached right now`
    );
  });

  it("dispatches exactly one retryNow per click and schedules nothing", () => {
    seed({ presentation: "error", blockedFailure: "version" });
    renderBanner();

    fireEvent.click(screen.getByTestId("remote-connection-banner-retry"));

    expect(retryActiveEnvironmentNow).toHaveBeenCalledTimes(1);
    expect(retryActiveEnvironmentNow).toHaveBeenCalledWith(REMOTE_ID);
  });

  it("routes the re-pair CTA to the Connections pane", () => {
    const openModal = vi.fn();
    seed({ presentation: "error", blockedFailure: "unauthorized" });
    useUiStore.setState({ openModal });
    renderBanner();

    fireEvent.click(screen.getByTestId("remote-connection-banner-repair"));

    expect(openModal).toHaveBeenCalledWith("settings", {
      section: "connections",
    });
  });
});
