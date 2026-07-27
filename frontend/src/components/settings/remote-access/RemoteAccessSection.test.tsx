/**
 * RemoteAccessSection tests (PR 1.7).
 *
 * Proof obligations: C-8 first-paint (shell before any invoke), flag inertness,
 * optimistic listener toggle with revert, pairing flow + countdown expiry,
 * agent-control warning-before-commit, teardown-backed revoke/disconnect,
 * local-only session event subscriptions, and explicit degraded states for the
 * missing PR 1.6 endpoint / audit surfaces.
 */

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  remoteHostApi,
  type MintedRemotePairingCode,
  type RemoteDeviceView,
  type RemoteListenerStatus,
  type RemoteSessionView,
} from "@/api/remote-host";
import { TooltipProvider } from "@/components/ui/tooltip";

import { RemoteAccessSection } from "./RemoteAccessSection";

// ---------------------------------------------------------------------------
// Mocks
// ---------------------------------------------------------------------------

const flagsState = vi.hoisted(() => ({ remoteEnvironments: true }));

vi.mock("@/hooks/useFeatureFlags", () => ({
  useFeatureFlags: () => ({
    data: { remoteEnvironments: flagsState.remoteEnvironments },
  }),
}));

const busState = vi.hoisted(() => {
  const handlers = new Map<string, Set<(payload: unknown) => void>>();
  return {
    handlers,
    emit(event: string, payload: unknown) {
      for (const handler of handlers.get(event) ?? []) {
        handler(payload);
      }
    },
    subscribe: undefined as unknown as ReturnType<typeof vi.fn>,
  };
});

vi.mock("@/providers/EventProvider", () => {
  const subscribe = vi.fn(
    (event: string, handler: (payload: unknown) => void) => {
      const set = busState.handlers.get(event) ?? new Set();
      set.add(handler);
      busState.handlers.set(event, set);
      const unsubscribe = () => {
        set.delete(handler);
      };
      return Object.assign(unsubscribe, { ready: Promise.resolve() });
    },
  );
  busState.subscribe = subscribe;
  return {
    useEventBus: () => ({ subscribe }),
  };
});

vi.mock("@/api/remote-host", () => ({
  REMOTE_SESSION_CONNECTED_EVENT: "remote:session_connected",
  REMOTE_SESSION_CLOSED_EVENT: "remote:session_closed",
  remoteHostApi: {
    getListenerStatus: vi.fn(),
    startListener: vi.fn(),
    stopListener: vi.fn(),
    setExposureMode: vi.fn(),
    generatePairingCode: vi.fn(),
    listPairingCodes: vi.fn(),
    revokePairingCode: vi.fn(),
    listDevices: vi.fn(),
    setDeviceAgentControl: vi.fn(),
    revokeDevice: vi.fn(),
    listSessions: vi.fn(),
    disconnectSession: vi.fn(),
    listAdvertisedEndpoints: vi.fn(),
    listAuditEntries: vi.fn(),
  },
}));

const api = vi.mocked(remoteHostApi);

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const baseStatus: RemoteListenerStatus = {
  enabled: true,
  exposureMode: "serve",
  port: 3849,
  environmentId: "env-1",
  running: true,
  bindAddress: "127.0.0.1:3849",
  serveActive: true,
  serveDegradedReason: null,
};

const deviceOff: RemoteDeviceView = {
  id: "dev-1",
  name: "Anca's iPhone",
  tokenPrefix: "rxd_live_AbCd",
  scopes: ["ui:read", "ui:operate"],
  agentControlGranted: false,
  createdAt: "2026-07-20T10:00:00Z",
  lastSeenAt: "2026-07-27T09:00:00Z",
  revokedAt: null,
  liveSessionCount: 1,
};

const deviceOn: RemoteDeviceView = {
  ...deviceOff,
  id: "dev-2",
  name: "Work MacBook",
  tokenPrefix: "rxd_live_EfGh",
  scopes: ["ui:read", "ui:operate", "ui:agent"],
  agentControlGranted: true,
  liveSessionCount: 0,
};

const session: RemoteSessionView = {
  id: "sess-1",
  deviceId: "dev-1",
  connectedAt: "2026-07-27T09:00:00Z",
  lastActiveAt: "2026-07-27T09:30:00Z",
  remoteAddr: "100.64.0.7:52001",
  live: true,
};

function minted(expiresAt: string): MintedRemotePairingCode {
  return {
    id: "pc-1",
    code: "rxp_ABCDEFGHJKLMNPQRSTUVWXYZabcdef01",
    scopes: ["ui:read", "ui:operate"],
    createdAt: "2026-07-27T10:00:00Z",
    expiresAt,
    expiresInSecs: 600,
  };
}

function renderSection() {
  return render(
    <TooltipProvider>
      <RemoteAccessSection />
    </TooltipProvider>,
  );
}

async function hydrate() {
  await waitFor(() => {
    expect(api.getListenerStatus).toHaveBeenCalled();
  });
  await waitFor(() => {
    expect(screen.getByTestId("remote-enable-toggle")).not.toBeDisabled();
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  busState.handlers.clear();
  flagsState.remoteEnvironments = true;
  api.getListenerStatus.mockResolvedValue(baseStatus);
  api.startListener.mockResolvedValue({ ...baseStatus, enabled: true, running: true });
  api.stopListener.mockResolvedValue({ ...baseStatus, enabled: false, running: false });
  api.setExposureMode.mockResolvedValue({ ...baseStatus, exposureMode: "tailnetDirect" });
  api.listAdvertisedEndpoints.mockResolvedValue([
    { kind: "loopbackServe", url: "https://mac-studio.tailnet.ts.net", available: true },
  ]);
  api.listDevices.mockResolvedValue([deviceOff, deviceOn]);
  api.listSessions.mockResolvedValue([session]);
  api.listPairingCodes.mockResolvedValue([]);
  api.listAuditEntries.mockResolvedValue([
    {
      id: 1,
      deviceId: "dev-1",
      action: "pairing_succeeded",
      detail: null,
      createdAt: "2026-07-27T09:00:00Z",
    },
  ]);
  api.generatePairingCode.mockResolvedValue(
    minted(new Date(Date.now() + 600_000).toISOString()),
  );
  api.revokePairingCode.mockResolvedValue(true);
  api.setDeviceAgentControl.mockImplementation((deviceId, enabled) =>
    Promise.resolve({
      ...(deviceId === deviceOff.id ? deviceOff : deviceOn),
      agentControlGranted: enabled,
    }),
  );
  api.revokeDevice.mockResolvedValue({
    ...deviceOff,
    revokedAt: "2026-07-27T11:00:00Z",
  });
  api.disconnectSession.mockResolvedValue(true);
});

afterEach(() => {
  vi.useRealTimers();
});

// ---------------------------------------------------------------------------
// Feature flag
// ---------------------------------------------------------------------------

describe("feature gating", () => {
  it("renders nothing and never invokes while remoteEnvironments is off", async () => {
    flagsState.remoteEnvironments = false;
    renderSection();
    expect(screen.queryByTestId("remote-access-section")).not.toBeInTheDocument();
    // Give any wrongly scheduled hydration time to fire.
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 80));
    });
    expect(api.getListenerStatus).not.toHaveBeenCalled();
    expect(api.listDevices).not.toHaveBeenCalled();
    expect(busState.subscribe).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// First paint (rule 24 / C-8)
// ---------------------------------------------------------------------------

describe("first paint", () => {
  it("paints the shell synchronously, before any backend invoke", async () => {
    renderSection();
    // Synchronous assertions — no awaits yet.
    expect(screen.getByTestId("remote-access-section")).toBeInTheDocument();
    expect(screen.getByText("Remote Access")).toBeInTheDocument();
    expect(screen.getByText("Pair a device")).toBeInTheDocument();
    expect(screen.getByText("Paired devices")).toBeInTheDocument();
    expect(screen.getByText("Live sessions")).toBeInTheDocument();
    expect(api.getListenerStatus).not.toHaveBeenCalled();
    expect(api.listDevices).not.toHaveBeenCalled();
    expect(api.listSessions).not.toHaveBeenCalled();
    expect(api.listAdvertisedEndpoints).not.toHaveBeenCalled();
    // Hydration happens after the paint boundary.
    await hydrate();
    expect(api.listDevices).toHaveBeenCalled();
    expect(api.listSessions).toHaveBeenCalled();
  });

  it("hydrates status, endpoints, devices, sessions, and audit entries", async () => {
    renderSection();
    await hydrate();
    expect(
      await screen.findByText("https://mac-studio.tailnet.ts.net"),
    ).toBeInTheDocument();
    expect(
      within(screen.getByTestId("remote-device-dev-1")).getByText("Anca's iPhone"),
    ).toBeInTheDocument();
    expect(
      within(screen.getByTestId("remote-device-dev-2")).getByText("Work MacBook"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("remote-session-sess-1")).toBeInTheDocument();
    expect(await screen.findByTestId("remote-audit")).toBeInTheDocument();
    expect(screen.getByText(/Pairing succeeded/)).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------------------
// Listener controls
// ---------------------------------------------------------------------------

describe("listener controls", () => {
  it("flips the enable toggle optimistically before the invoke settles", async () => {
    api.getListenerStatus.mockResolvedValue({
      ...baseStatus,
      enabled: false,
      running: false,
    });
    let resolveStart: ((status: RemoteListenerStatus) => void) | undefined;
    api.startListener.mockImplementation(
      () =>
        new Promise<RemoteListenerStatus>((resolve) => {
          resolveStart = resolve;
        }),
    );
    renderSection();
    await hydrate();

    const toggle = screen.getByTestId("remote-enable-toggle");
    expect(toggle).toHaveAttribute("aria-checked", "false");
    fireEvent.click(toggle);
    // Optimistic: checked before the promise resolves.
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(api.startListener).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolveStart?.({ ...baseStatus, enabled: true, running: true });
    });
    await waitFor(() => {
      expect(toggle).toHaveAttribute("aria-checked", "true");
    });
  });

  it("reverts the toggle and surfaces the error when the invoke fails", async () => {
    api.getListenerStatus.mockResolvedValue({
      ...baseStatus,
      enabled: false,
      running: false,
    });
    api.startListener.mockRejectedValue(new Error("port already in use"));
    renderSection();
    await hydrate();

    const toggle = screen.getByTestId("remote-enable-toggle");
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-checked", "true");
    await waitFor(() => {
      expect(toggle).toHaveAttribute("aria-checked", "false");
    });
    expect(screen.getByTestId("remote-access-error")).toHaveTextContent(
      "port already in use",
    );
  });

  it("switches exposure mode optimistically via set_remote_exposure_mode", async () => {
    renderSection();
    await hydrate();

    const tailnet = screen.getByTestId("remote-mode-tailnetDirect");
    expect(tailnet).toHaveAttribute("aria-checked", "false");
    fireEvent.click(tailnet);
    expect(tailnet).toHaveAttribute("aria-checked", "true");
    expect(api.setExposureMode).toHaveBeenCalledWith("tailnetDirect");
  });

  it("shows the serve degraded reason from the listener status", async () => {
    api.getListenerStatus.mockResolvedValue({
      ...baseStatus,
      serveActive: false,
      serveDegradedReason: "tailscale is not logged in",
    });
    renderSection();
    await hydrate();
    expect(screen.getByTestId("remote-serve-degraded")).toHaveTextContent(
      "tailscale is not logged in",
    );
  });

  it("shows an explicit degraded note when endpoint discovery is unavailable", async () => {
    api.listAdvertisedEndpoints.mockRejectedValue(
      new Error("unknown command list_remote_advertised_endpoints"),
    );
    renderSection();
    await hydrate();
    expect(
      await screen.findByTestId("remote-endpoints-unavailable"),
    ).toBeInTheDocument();
  });
});

// ---------------------------------------------------------------------------
// Pairing flow
// ---------------------------------------------------------------------------

describe("pairing flow", () => {
  it("mints a code and shows the grouped code + hash-fragment URL", async () => {
    renderSection();
    await hydrate();

    fireEvent.click(screen.getByTestId("remote-pair-device"));
    expect(api.generatePairingCode).toHaveBeenCalledTimes(1);

    const card = await screen.findByTestId("remote-pairing-card");
    expect(within(card).getByTestId("remote-pairing-code")).toHaveTextContent(
      "rxp_ABCD EFGH JKLM NPQR STUV WXYZ abcd ef01",
    );
    // Preferred endpoint (R-12): the single advertised endpoint; code in the fragment.
    expect(
      within(card).getByTestId("remote-pairing-url-value"),
    ).toHaveTextContent(
      "ralphx://pair?host=https%3A%2F%2Fmac-studio.tailnet.ts.net#code=rxp_ABCDEFGHJKLMNPQRSTUVWXYZabcdef01",
    );
    expect(
      within(card).getByTestId("remote-pairing-countdown").textContent,
    ).toMatch(/Expires in (10:00|9:5\d)/);
  });

  it("cancels the displayed code through revoke_remote_pairing_code", async () => {
    renderSection();
    await hydrate();
    fireEvent.click(screen.getByTestId("remote-pair-device"));
    const card = await screen.findByTestId("remote-pairing-card");

    fireEvent.click(within(card).getByTestId("remote-pairing-cancel"));
    expect(api.revokePairingCode).toHaveBeenCalledWith("pc-1");
    expect(screen.queryByTestId("remote-pairing-card")).not.toBeInTheDocument();
  });

  it("cancels an outstanding code from the list", async () => {
    api.listPairingCodes.mockResolvedValue([
      {
        id: "pc-9",
        scopes: ["ui:read"],
        createdAt: "2026-07-27T09:55:00Z",
        expiresAt: "2026-07-27T10:05:00Z",
      },
    ]);
    renderSection();
    await hydrate();

    fireEvent.click(await screen.findByTestId("remote-code-cancel-pc-9"));
    expect(api.revokePairingCode).toHaveBeenCalledWith("pc-9");
  });

  it("expires the code when the countdown reaches zero", async () => {
    vi.useFakeTimers({
      toFake: [
        "setTimeout",
        "clearTimeout",
        "setInterval",
        "clearInterval",
        "requestAnimationFrame",
        "cancelAnimationFrame",
        "Date",
      ],
    });
    vi.setSystemTime(new Date("2026-07-27T10:00:00Z"));
    api.generatePairingCode.mockResolvedValue(minted("2026-07-27T10:10:00Z"));

    renderSection();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(100);
    });

    fireEvent.click(screen.getByTestId("remote-pair-device"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(50);
    });
    // 150ms of fake time elapsed since mint (hydrate + settle), so 599s remain.
    expect(screen.getByTestId("remote-pairing-countdown")).toHaveTextContent(
      "Expires in 9:59",
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(screen.getByTestId("remote-pairing-countdown")).toHaveTextContent(
      "Expires in 8:59",
    );

    const outstandingCallsBeforeExpiry = api.listPairingCodes.mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(9 * 60_000);
    });
    expect(screen.getByTestId("remote-pairing-expired")).toBeInTheDocument();
    expect(screen.queryByTestId("remote-pairing-card")).not.toBeInTheDocument();
    // Expiry refreshes the outstanding-codes list exactly once for this code.
    expect(api.listPairingCodes.mock.calls.length).toBe(
      outstandingCallsBeforeExpiry + 1,
    );
  });
});

// ---------------------------------------------------------------------------
// Agent control (the one deliberate consent)
// ---------------------------------------------------------------------------

describe("agent control toggle", () => {
  it("shows the explicit warning before committing a grant", async () => {
    renderSection();
    await hydrate();

    fireEvent.click(screen.getByTestId("remote-agent-control-dev-1"));
    // No commit before consent.
    expect(api.setDeviceAgentControl).not.toHaveBeenCalled();

    const warning = await screen.findByTestId("remote-agent-warning");
    expect(warning).toHaveTextContent(/kanban/i);
    expect(warning).toHaveTextContent(/start and steer agents/i);
    expect(warning).toHaveTextContent(/inject tasks into the ready queue/i);
    expect(warning).toHaveTextContent(/code execution on this Mac/i);

    fireEvent.click(screen.getByTestId("remote-agent-warning-confirm"));
    expect(api.setDeviceAgentControl).toHaveBeenCalledWith("dev-1", true);
    await waitFor(() => {
      expect(screen.getByTestId("remote-agent-control-dev-1")).toHaveAttribute(
        "aria-checked",
        "true",
      );
    });
  });

  it("does not grant when the warning is cancelled", async () => {
    renderSection();
    await hydrate();

    fireEvent.click(screen.getByTestId("remote-agent-control-dev-1"));
    await screen.findByTestId("remote-agent-warning");
    fireEvent.click(screen.getByTestId("remote-agent-warning-cancel"));
    expect(api.setDeviceAgentControl).not.toHaveBeenCalled();
    expect(screen.getByTestId("remote-agent-control-dev-1")).toHaveAttribute(
      "aria-checked",
      "false",
    );
  });

  it("withdraws immediately without a dialog and refreshes sessions (teardown)", async () => {
    renderSection();
    await hydrate();
    const sessionsCallsBefore = api.listSessions.mock.calls.length;

    fireEvent.click(screen.getByTestId("remote-agent-control-dev-2"));
    expect(screen.queryByTestId("remote-agent-warning")).not.toBeInTheDocument();
    expect(api.setDeviceAgentControl).toHaveBeenCalledWith("dev-2", false);
    // Optimistic: off before the invoke settles.
    expect(screen.getByTestId("remote-agent-control-dev-2")).toHaveAttribute(
      "aria-checked",
      "false",
    );
    // Withdrawal fires kill channels — the session list must re-prove itself.
    await waitFor(() => {
      expect(api.listSessions.mock.calls.length).toBeGreaterThan(sessionsCallsBefore);
    });
  });

  it("reverts the optimistic grant when the backend rejects it", async () => {
    api.setDeviceAgentControl.mockRejectedValue(new Error("device revoked"));
    renderSection();
    await hydrate();

    fireEvent.click(screen.getByTestId("remote-agent-control-dev-1"));
    fireEvent.click(await screen.findByTestId("remote-agent-warning-confirm"));
    await waitFor(() => {
      expect(screen.getByTestId("remote-agent-control-dev-1")).toHaveAttribute(
        "aria-checked",
        "false",
      );
    });
    expect(screen.getByTestId("remote-access-error")).toHaveTextContent(
      "device revoked",
    );
  });
});

// ---------------------------------------------------------------------------
// Revoke + sessions
// ---------------------------------------------------------------------------

describe("revoke and sessions", () => {
  it("revokes a device through the teardown-backed command after confirm", async () => {
    renderSection();
    await hydrate();

    fireEvent.click(screen.getByTestId("remote-device-revoke-dev-1"));
    expect(api.revokeDevice).not.toHaveBeenCalled();
    fireEvent.click(
      await screen.findByTestId("remote-device-revoke-confirm-action"),
    );
    expect(api.revokeDevice).toHaveBeenCalledWith("dev-1");

    const row = await screen.findByTestId("remote-device-dev-1");
    await waitFor(() => {
      expect(within(row).getByText("Revoked")).toBeInTheDocument();
    });
    expect(
      within(row).queryByTestId("remote-agent-control-dev-1"),
    ).not.toBeInTheDocument();
  });

  it("disconnects a session immediately (row leaves before the invoke settles)", async () => {
    let resolveDisconnect: ((value: boolean) => void) | undefined;
    api.disconnectSession.mockImplementation(
      () =>
        new Promise<boolean>((resolve) => {
          resolveDisconnect = resolve;
        }),
    );
    renderSection();
    await hydrate();
    await screen.findByTestId("remote-session-sess-1");

    api.listSessions.mockResolvedValue([]);
    fireEvent.click(screen.getByTestId("remote-session-disconnect-sess-1"));
    // Immediate: no waiting on the backend for the visual removal.
    expect(screen.queryByTestId("remote-session-sess-1")).not.toBeInTheDocument();
    expect(api.disconnectSession).toHaveBeenCalledWith("sess-1");
    await act(async () => {
      resolveDisconnect?.(true);
    });
  });

  it("subscribes to the local-only session events and refreshes on them", async () => {
    renderSection();
    await hydrate();
    await waitFor(() => {
      expect(busState.subscribe).toHaveBeenCalledWith(
        "remote:session_connected",
        expect.any(Function),
      );
      expect(busState.subscribe).toHaveBeenCalledWith(
        "remote:session_closed",
        expect.any(Function),
      );
    });

    const second: RemoteSessionView = {
      ...session,
      id: "sess-2",
      remoteAddr: "100.64.0.9:40100",
    };
    api.listSessions.mockResolvedValue([session, second]);
    act(() => {
      busState.emit("remote:session_connected", { sessionId: "sess-2" });
    });
    expect(await screen.findByTestId("remote-session-sess-2")).toBeInTheDocument();
  });

  it("shows the explicit audit-unavailable note when the audit surface is missing", async () => {
    api.listAuditEntries.mockRejectedValue(
      new Error("unknown command list_remote_audit_entries"),
    );
    renderSection();
    await hydrate();
    expect(await screen.findByTestId("remote-audit-unavailable")).toBeInTheDocument();
  });
});
