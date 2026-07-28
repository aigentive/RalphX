import { act, fireEvent, render as rtlRender, screen, waitFor } from "@testing-library/react";
import type { ReactElement } from "react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";

import { AddEnvironmentDialog } from "./AddEnvironmentDialog";

const { previewMock, pairMock, listMock, setActiveMock } = vi.hoisted(() => ({
  previewMock: vi.fn(),
  pairMock: vi.fn(),
  listMock: vi.fn(),
  setActiveMock: vi.fn(),
}));

vi.mock("@/api/remote-environments", () => ({
  remoteEnvironmentsApi: {
    preview: previewMock,
    pair: pairMock,
    list: listMock,
    setActiveEnvironment: setActiveMock,
    getActiveEnvironment: vi.fn(),
    remove: vi.fn(),
  },
}));

function preview(overrides: Record<string, unknown> = {}) {
  return {
    environmentId: "a1b2c3d4e5f6g7h8f9",
    appVersion: "0.9.4",
    platform: "macOS",
    protocolVersion: 1,
    minClientProtocol: 1,
    alreadyPairedAs: null,
    ...overrides,
  };
}

function summary(overrides: Record<string, unknown> = {}) {
  return {
    id: "row-1",
    environmentId: "a1b2c3d4e5f6g7h8f9",
    name: "Studio Mac",
    baseUrl: "https://studio.tail-x.ts.net:3849",
    candidateUrls: [],
    scopes: ["ui:read"],
    protocolVersion: 1,
    status: "active" as const,
    createdAt: "2026-07-28T00:00:00Z",
    lastConnectedAt: null,
    ...overrides,
  };
}

/** The app mounts one global TooltipProvider (App.tsx); tests supply their own. */
function render(ui: ReactElement) {
  return rtlRender(<TooltipProvider>{ui}</TooltipProvider>);
}

const PAIRING_URL =
  "ralphx://pair?host=https%3A%2F%2Fstudio.tail-x.ts.net%3A3849#code=rxp_ABCD1234EFGH";

function resetStore(): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
    environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
    connectionStates: { [LOCAL_ENVIRONMENT_ID]: "connected" },
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  previewMock.mockResolvedValue(preview());
  pairMock.mockResolvedValue(summary());
  listMock.mockResolvedValue([summary()]);
  setActiveMock.mockResolvedValue(null);
  resetStore();
});

afterEach(() => {
  resetStore();
});

/** Walks the wizard to the verify step with a pasted pairing link. */
async function reachVerifyStep(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByTestId("add-environment-host"));
  await user.paste(PAIRING_URL);
  await user.click(screen.getByTestId("add-environment-continue"));
  await screen.findByTestId("add-environment-step-verify");
}

describe("AddEnvironmentDialog — first paint (rule 24)", () => {
  it("paints the dialog shell before any invoke is dispatched", () => {
    render(<AddEnvironmentDialog open onOpenChange={() => {}} />);

    // The shell exists on the opening commit; nothing was fetched to produce it.
    expect(screen.getByTestId("add-environment-dialog")).toBeInTheDocument();
    expect(screen.getByTestId("add-environment-step-connect")).toBeInTheDocument();
    expect(previewMock).not.toHaveBeenCalled();
    expect(pairMock).not.toHaveBeenCalled();
    expect(listMock).not.toHaveBeenCalled();
  });

  it("fetches nothing until the user submits the first step", async () => {
    const user = userEvent.setup();
    render(<AddEnvironmentDialog open onOpenChange={() => {}} />);

    await user.click(screen.getByTestId("add-environment-host"));
    await user.paste(PAIRING_URL);

    // Typing/pasting is not a submission: a paste must never consume a code.
    expect(previewMock).not.toHaveBeenCalled();
  });
});

describe("AddEnvironmentDialog — step 1 input", () => {
  it("fills host AND code from a pasted pairing link, rendering the code grouped", async () => {
    const user = userEvent.setup();
    render(<AddEnvironmentDialog open onOpenChange={() => {}} />);

    await user.click(screen.getByTestId("add-environment-host"));
    await user.paste(PAIRING_URL);

    expect(screen.getByTestId("add-environment-host")).toHaveValue(
      "https://studio.tail-x.ts.net:3849",
    );
    expect(screen.getByTestId("add-environment-code")).toHaveValue(
      "rxp_ ABCD 1234 EFGH",
    );
  });

  it("refuses a link whose code rode in the query string, inline on the field", async () => {
    const user = userEvent.setup();
    render(<AddEnvironmentDialog open onOpenChange={() => {}} />);

    await user.click(screen.getByTestId("add-environment-host"));
    await user.paste(
      "ralphx://pair?host=https%3A%2F%2Fh.ts.net%3A3849&code=rxp_LEAKED",
    );

    expect(screen.getByTestId("add-environment-host-error")).toHaveTextContent(
      /query string/i,
    );
    // The burned code was NOT adopted into the form.
    expect(screen.getByTestId("add-environment-code")).toHaveValue("");
  });

  it("keeps Continue disabled until both host and code validate locally", async () => {
    const user = userEvent.setup();
    render(<AddEnvironmentDialog open onOpenChange={() => {}} />);
    const button = screen.getByTestId("add-environment-continue");

    expect(button).toBeDisabled();
    await user.type(screen.getByTestId("add-environment-host"), "studio.ts.net:3849");
    expect(button).toBeDisabled();

    await user.type(screen.getByTestId("add-environment-code"), "nope1234");
    expect(button).toBeDisabled();

    await user.clear(screen.getByTestId("add-environment-code"));
    await user.type(screen.getByTestId("add-environment-code"), "rxp_ABCD1234");
    expect(button).toBeEnabled();
  });

  it("accepts a manually typed grouped code", async () => {
    const user = userEvent.setup();
    render(<AddEnvironmentDialog open onOpenChange={() => {}} />);

    await user.type(screen.getByTestId("add-environment-host"), "studio.ts.net:3849");
    await user.click(screen.getByTestId("add-environment-code"));
    await user.paste("rxp_ ABCD 1234");
    await user.click(screen.getByTestId("add-environment-continue"));

    await waitFor(() => expect(previewMock).toHaveBeenCalledTimes(1));
    expect(previewMock).toHaveBeenCalledWith("https://studio.ts.net:3849");
  });
});

describe("AddEnvironmentDialog — step 2 verify", () => {
  it("renders exactly the descriptor fields, and no project count", async () => {
    const user = userEvent.setup();
    render(<AddEnvironmentDialog open onOpenChange={() => {}} />);
    await reachVerifyStep(user);

    expect(screen.getByTestId("add-environment-identity")).toHaveTextContent("a1b2c3…h8f9");
    expect(screen.getByText("0.9.4")).toBeInTheDocument();
    expect(screen.getByText("macOS")).toBeInTheDocument();
    expect(screen.getByTestId("add-environment-protocol")).toHaveTextContent("v1");
    // The wire descriptor has no project count; inventing one would be a lie.
    expect(screen.queryByText(/project/i)).not.toBeInTheDocument();
  });

  it("says re-pairing UPDATES an already-paired host", async () => {
    previewMock.mockResolvedValue(preview({ alreadyPairedAs: "Studio Mac" }));
    const user = userEvent.setup();
    render(<AddEnvironmentDialog open onOpenChange={() => {}} />);
    await reachVerifyStep(user);

    expect(screen.getByTestId("add-environment-already-paired")).toHaveTextContent(
      /updates it rather than adding a second entry/i,
    );
    expect(screen.getByTestId("add-environment-name")).toHaveValue("Studio Mac");
  });

  it("prefills the name from the host when the host is unknown", async () => {
    const user = userEvent.setup();
    render(<AddEnvironmentDialog open onOpenChange={() => {}} />);
    await reachVerifyStep(user);

    expect(screen.getByTestId("add-environment-name")).toHaveValue(
      "studio.tail-x.ts.net:3849",
    );
  });

  it("blocks Pair on an empty name", async () => {
    const user = userEvent.setup();
    render(<AddEnvironmentDialog open onOpenChange={() => {}} />);
    await reachVerifyStep(user);

    await user.clear(screen.getByTestId("add-environment-name"));
    expect(screen.getByTestId("add-environment-pair")).toBeDisabled();
    expect(pairMock).not.toHaveBeenCalled();
  });
});

describe("AddEnvironmentDialog — version contradiction", () => {
  it("parks in blocked with both versions and never retries on its own (A-5)", async () => {
    previewMock.mockRejectedValue(
      new Error(
        "REMOTE_VERSION_MISMATCH: host requires client protocol >= 2, this client speaks 1",
      ),
    );
    render(<AddEnvironmentDialog open onOpenChange={() => {}} />);

    // fireEvent + fake timers, deliberately: userEvent and testing-library's `waitFor`
    // both schedule real-timer pollers of their own, which would mask the thing under
    // assertion — work this feature schedules for later.
    fireEvent.change(screen.getByTestId("add-environment-host"), {
      target: { value: "https://studio.tail-x.ts.net:3849" },
    });
    fireEvent.change(screen.getByTestId("add-environment-code"), {
      target: { value: "rxp_ABCD1234EFGH" },
    });

    vi.useFakeTimers();
    try {
      fireEvent.click(screen.getByTestId("add-environment-continue"));
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
      });

      const banner = screen.getByTestId("add-environment-blocked-banner");
      expect(banner).toHaveTextContent("Versions are incompatible");
      expect(banner).toHaveTextContent("host requires client protocol >= 2");
      expect(banner).toHaveTextContent("this client speaks 1");
      expect(previewMock).toHaveBeenCalledTimes(1);

      // Run every pending timer, repeatedly, well past any plausible backoff ladder.
      // The supervisor is the sole retry owner (A-5); this feature must sit still.
      await act(async () => {
        vi.advanceTimersByTime(120_000);
        await Promise.resolve();
      });

      expect(previewMock).toHaveBeenCalledTimes(1);
      expect(pairMock).not.toHaveBeenCalled();
      // And it is still parked in blocked, not silently re-entered.
      expect(screen.getByTestId("add-environment-blocked")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("returns to step 1 from blocked", async () => {
    previewMock.mockRejectedValue(
      new Error("REMOTE_VERSION_MISMATCH: host requires client protocol >= 2, client 1"),
    );
    const user = userEvent.setup();
    render(<AddEnvironmentDialog open onOpenChange={() => {}} />);

    await user.click(screen.getByTestId("add-environment-host"));
    await user.paste(PAIRING_URL);
    await user.click(screen.getByTestId("add-environment-continue"));
    await user.click(await screen.findByTestId("add-environment-blocked-back"));

    expect(screen.getByTestId("add-environment-step-connect")).toBeInTheDocument();
  });
});

describe("AddEnvironmentDialog — failures", () => {
  it("renders an unreachable host as an actionable error, not a blocked state", async () => {
    previewMock.mockRejectedValue(
      new Error("REMOTE_UNREACHABLE: host unreachable: offline"),
    );
    const user = userEvent.setup();
    render(<AddEnvironmentDialog open onOpenChange={() => {}} />);

    await user.click(screen.getByTestId("add-environment-host"));
    await user.paste(PAIRING_URL);
    await user.click(screen.getByTestId("add-environment-continue"));

    expect(await screen.findByTestId("add-environment-error-banner")).toHaveTextContent(
      /could not reach the host/i,
    );
    expect(screen.queryByTestId("add-environment-blocked")).not.toBeInTheDocument();
  });

  it("tells the user to generate a fresh code when pairing is rejected", async () => {
    pairMock.mockRejectedValue(new Error("PAIRING_REJECTED: code already used"));
    const user = userEvent.setup();
    render(<AddEnvironmentDialog open onOpenChange={() => {}} />);
    await reachVerifyStep(user);
    await user.click(screen.getByTestId("add-environment-pair"));

    expect(await screen.findByTestId("add-environment-error-banner")).toHaveTextContent(
      /fresh code/i,
    );
    // A failed pair must not report success anywhere.
    expect(screen.queryByTestId("add-environment-success")).not.toBeInTheDocument();
  });

  it("does not refresh the registry when pairing failed", async () => {
    pairMock.mockRejectedValue(new Error("PAIRING_REJECTED: nope"));
    const user = userEvent.setup();
    render(<AddEnvironmentDialog open onOpenChange={() => {}} />);
    await reachVerifyStep(user);
    await user.click(screen.getByTestId("add-environment-pair"));
    await screen.findByTestId("add-environment-error");

    expect(listMock).not.toHaveBeenCalled();
  });
});

describe("AddEnvironmentDialog — pairing", () => {
  it("pairs with the RAW code and the normalized host, then refreshes the registry", async () => {
    const onPaired = vi.fn();
    const user = userEvent.setup();
    render(<AddEnvironmentDialog open onOpenChange={() => {}} onPaired={onPaired} />);
    await reachVerifyStep(user);
    await user.click(screen.getByTestId("add-environment-pair"));

    await screen.findByTestId("add-environment-success");
    expect(pairMock).toHaveBeenCalledWith(
      "https://studio.tail-x.ts.net:3849",
      "rxp_ABCD1234EFGH", // raw, never the grouped display form
      "studio.tail-x.ts.net:3849",
    );
    expect(listMock).toHaveBeenCalledTimes(1);
    expect(onPaired).toHaveBeenCalledTimes(1);
    expect(
      useEnvironmentStore.getState().environments.map((entry) => entry.id),
    ).toContain("row-1");
  });

  it("cannot be dismissed while the staged Rust sequence is in flight", async () => {
    let releasePair: (value: unknown) => void = () => {};
    pairMock.mockImplementation(
      () =>
        new Promise((resolve) => {
          releasePair = resolve;
        }),
    );
    const onOpenChange = vi.fn();
    const user = userEvent.setup();
    render(<AddEnvironmentDialog open onOpenChange={onOpenChange} />);
    await reachVerifyStep(user);
    await user.click(screen.getByTestId("add-environment-pair"));

    await user.keyboard("{Escape}");
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(screen.getByTestId("add-environment-pair")).toBeDisabled();

    releasePair(summary());
    await screen.findByTestId("add-environment-success");
  });

  it("switches to the new environment on demand and closes", async () => {
    const onOpenChange = vi.fn();
    const user = userEvent.setup();
    render(<AddEnvironmentDialog open onOpenChange={onOpenChange} />);
    await reachVerifyStep(user);
    await user.click(screen.getByTestId("add-environment-pair"));
    await user.click(await screen.findByTestId("add-environment-switch"));

    await waitFor(() => expect(setActiveMock).toHaveBeenCalledWith("row-1"));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("Done closes without switching", async () => {
    const onOpenChange = vi.fn();
    const user = userEvent.setup();
    render(<AddEnvironmentDialog open onOpenChange={onOpenChange} />);
    await reachVerifyStep(user);
    await user.click(screen.getByTestId("add-environment-pair"));
    await user.click(await screen.findByTestId("add-environment-done"));

    expect(setActiveMock).not.toHaveBeenCalled();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});

describe("AddEnvironmentDialog — re-pair", () => {
  it("locks the host and prefills the name from the existing row", async () => {
    previewMock.mockResolvedValue(preview({ alreadyPairedAs: "Studio Mac" }));
    const user = userEvent.setup();
    render(
      <AddEnvironmentDialog
        open
        onOpenChange={() => {}}
        lockedHost="https://studio.tail-x.ts.net:3849"
        initialName="Studio Mac"
      />,
    );

    const host = screen.getByTestId("add-environment-host");
    expect(host).toHaveValue("https://studio.tail-x.ts.net:3849");
    expect(host).toBeDisabled();

    await user.type(screen.getByTestId("add-environment-code"), "rxp_ABCD1234");
    await user.click(screen.getByTestId("add-environment-continue"));
    await screen.findByTestId("add-environment-step-verify");

    expect(screen.getByTestId("add-environment-name")).toHaveValue("Studio Mac");
  });

  it("re-pairing a known host leaves ONE row — the upsert is visible end to end", async () => {
    previewMock.mockResolvedValue(preview({ alreadyPairedAs: "Studio Mac" }));
    // Rust upserts on environmentId, so the refreshed list still has one row.
    listMock.mockResolvedValue([summary({ name: "Studio Mac" })]);
    const user = userEvent.setup();
    render(
      <AddEnvironmentDialog
        open
        onOpenChange={() => {}}
        lockedHost="https://studio.tail-x.ts.net:3849"
        initialName="Studio Mac"
      />,
    );

    await user.type(screen.getByTestId("add-environment-code"), "rxp_ABCD1234");
    await user.click(screen.getByTestId("add-environment-continue"));
    await screen.findByTestId("add-environment-step-verify");
    await user.click(screen.getByTestId("add-environment-pair"));
    await screen.findByTestId("add-environment-success");

    const remote = useEnvironmentStore
      .getState()
      .environments.filter((entry) => entry.id !== LOCAL_ENVIRONMENT_ID);
    expect(remote).toHaveLength(1);
  });
});

describe("AddEnvironmentDialog — P-18", () => {
  it("never renders anything token-shaped", async () => {
    const user = userEvent.setup();
    const { container } = render(
      <AddEnvironmentDialog open onOpenChange={() => {}} />,
    );
    await reachVerifyStep(user);

    expect(container.textContent).not.toMatch(/rxd_|token|bearer|secret/i);
  });
});
