import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  render as rtlRender,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { RemoteEnvironmentSummary } from "@/api/remote-environments";
import { TooltipProvider } from "@/components/ui/tooltip";
import {
  LOCAL_ENVIRONMENT_ID,
  useEnvironmentStore,
} from "@/stores/environmentStore";

import { ConnectionsSection } from "./ConnectionsSection";

const { listMock, removeMock, pairMock, previewMock, flagsMock } = vi.hoisted(
  () => ({
    listMock: vi.fn(),
    removeMock: vi.fn(),
    pairMock: vi.fn(),
    previewMock: vi.fn(),
    flagsMock: vi.fn(),
  }),
);

vi.mock("@/api/remote-environments", () => ({
  remoteEnvironmentsApi: {
    list: listMock,
    remove: removeMock,
    pair: pairMock,
    preview: previewMock,
    getActiveEnvironment: vi.fn(),
    setActiveEnvironment: vi.fn(),
  },
}));

vi.mock("@/hooks/useFeatureFlags", () => ({
  useFeatureFlags: () => flagsMock(),
}));

function summary(
  overrides: Partial<RemoteEnvironmentSummary> = {},
): RemoteEnvironmentSummary {
  return {
    id: "row-1",
    environmentId: "host-1",
    name: "Studio Mac",
    baseUrl: "https://studio.tail-x.ts.net:3849",
    candidateUrls: [],
    scopes: ["ui:read"],
    protocolVersion: 1,
    status: "active",
    createdAt: "2026-07-28T00:00:00Z",
    lastConnectedAt: null,
    ...overrides,
  };
}

function render(ui: ReactElement) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return rtlRender(
    <QueryClientProvider client={client}>
      <TooltipProvider>{ui}</TooltipProvider>
    </QueryClientProvider>,
  );
}

function resetStore(): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
    ],
    connectionStates: { [LOCAL_ENVIRONMENT_ID]: "connected" },
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  flagsMock.mockReturnValue({ data: { remoteEnvironments: true } });
  listMock.mockResolvedValue([summary()]);
  removeMock.mockResolvedValue(null);
  localStorage.clear();
  resetStore();
});

afterEach(() => {
  resetStore();
  localStorage.clear();
});

describe("ConnectionsSection — dark ship", () => {
  it("renders nothing and fires no invoke while the flag is off", async () => {
    flagsMock.mockReturnValue({ data: { remoteEnvironments: false } });
    const { container } = render(<ConnectionsSection />);

    expect(container).toBeEmptyDOMElement();
    await Promise.resolve();
    expect(listMock).not.toHaveBeenCalled();
  });
});

describe("ConnectionsSection — first paint (rule 24)", () => {
  it("paints header and skeleton before the list request is dispatched", () => {
    render(<ConnectionsSection />);

    // The shell is on screen in the mounting commit; the fetch waits for the boundary.
    expect(screen.getByTestId("connections-section")).toBeInTheDocument();
    expect(screen.getByText("Connections")).toBeInTheDocument();
    expect(listMock).not.toHaveBeenCalled();
  });

  it("fetches the list only after the paint boundary", async () => {
    render(<ConnectionsSection />);
    await waitFor(() => expect(listMock).toHaveBeenCalledTimes(1));
  });
});

describe("ConnectionsSection — rows", () => {
  it("shows an active row as Paired, not as a live connection", async () => {
    render(<ConnectionsSection />);
    await screen.findByTestId("connections-row-row-1");

    // This pane knows the registry, not the socket.
    expect(screen.getByTestId("connections-status-row-1")).toHaveTextContent(
      "Paired",
    );
    expect(screen.queryByText(/connected/i)).not.toBeInTheDocument();
  });

  it("explains a pending_delete row and offers NO actions", async () => {
    listMock.mockResolvedValue([summary({ status: "pending_delete" })]);
    render(<ConnectionsSection />);
    const row = await screen.findByTestId("connections-row-row-1");

    expect(screen.getByTestId("connections-status-row-1")).toHaveTextContent(
      "Removing…",
    );
    expect(
      screen.getByTestId("connections-explanation-row-1"),
    ).toHaveTextContent(/finish automatically/i);
    // The reconciler owns this row; offering a lifecycle action would invite double
    // work. (The address copy button stays — reading is not acting.)
    expect(
      within(row).queryByTestId("connections-remove-row-1"),
    ).not.toBeInTheDocument();
    expect(
      within(row).queryByTestId("connections-repair-row-1"),
    ).not.toBeInTheDocument();
  });

  it("offers a finish CTA on a pending_add husk", async () => {
    listMock.mockResolvedValue([summary({ status: "pending_add" })]);
    render(<ConnectionsSection />);
    await screen.findByTestId("connections-row-row-1");

    expect(screen.getByTestId("connections-status-row-1")).toHaveTextContent(
      "Finishing setup…",
    );
    expect(screen.getByTestId("connections-repair-row-1")).toHaveTextContent(
      "Re-pair to finish",
    );
    expect(screen.getByTestId("connections-remove-row-1")).toBeInTheDocument();
  });

  it("renders the empty state with its own add affordance", async () => {
    listMock.mockResolvedValue([]);
    render(<ConnectionsSection />);

    expect(await screen.findByTestId("connections-empty")).toHaveTextContent(
      /No remote environments yet/i,
    );
  });

  it("always shows the unremovable local environment", async () => {
    render(<ConnectionsSection />);
    expect(await screen.findByTestId("connections-local")).toHaveTextContent(
      /always available/i,
    );
  });

  it("gives the icon-only remove button an accessible name (rule 23)", async () => {
    render(<ConnectionsSection />);
    await screen.findByTestId("connections-row-row-1");

    expect(screen.getByTestId("connections-remove-row-1")).toHaveAccessibleName(
      "Remove Studio Mac",
    );
  });
});

describe("ConnectionsSection — removal", () => {
  it("confirms first, then stages the removal without vanishing the row", async () => {
    listMock
      .mockResolvedValueOnce([summary()])
      .mockResolvedValue([summary({ status: "pending_delete" })]);
    const user = userEvent.setup();
    render(<ConnectionsSection />);
    await screen.findByTestId("connections-row-row-1");

    await user.click(screen.getByTestId("connections-remove-row-1"));
    expect(
      await screen.findByTestId("connections-remove-confirm"),
    ).toBeInTheDocument();
    expect(removeMock).not.toHaveBeenCalled();

    await user.click(screen.getByTestId("connections-remove-confirm-action"));

    await waitFor(() =>
      expect(screen.getByTestId("connections-status-row-1")).toHaveTextContent(
        "Removing…",
      ),
    );
    // Still present: removal is staged, and the row leaves when Rust says it has.
    expect(screen.getByTestId("connections-row-row-1")).toBeInTheDocument();
  });

  it("cancelling the confirm removes nothing", async () => {
    const user = userEvent.setup();
    render(<ConnectionsSection />);
    await screen.findByTestId("connections-row-row-1");

    await user.click(screen.getByTestId("connections-remove-row-1"));
    await user.click(await screen.findByTestId("connections-remove-cancel"));

    expect(removeMock).not.toHaveBeenCalled();
  });

  it("clears the environment's scoped view state once removal is accepted (P-27)", async () => {
    localStorage.setItem(
      "ralphx-project-store:row-1",
      JSON.stringify({ state: {} }),
    );
    localStorage.setItem(
      "ralphx-project-store:row-2",
      JSON.stringify({ state: {} }),
    );
    const user = userEvent.setup();
    render(<ConnectionsSection />);
    await screen.findByTestId("connections-row-row-1");

    await user.click(screen.getByTestId("connections-remove-row-1"));
    await user.click(
      await screen.findByTestId("connections-remove-confirm-action"),
    );

    await waitFor(() =>
      expect(localStorage.getItem("ralphx-project-store:row-1")).toBeNull(),
    );
    // Another environment's slice is untouched.
    expect(localStorage.getItem("ralphx-project-store:row-2")).not.toBeNull();
  });

  it("keeps the scoped state when the backend refused the removal", async () => {
    localStorage.setItem(
      "ralphx-project-store:row-1",
      JSON.stringify({ state: {} }),
    );
    removeMock.mockRejectedValue(new Error("DATABASE_ERROR: boom"));
    const user = userEvent.setup();
    render(<ConnectionsSection />);
    await screen.findByTestId("connections-row-row-1");

    await user.click(screen.getByTestId("connections-remove-row-1"));
    await user.click(
      await screen.findByTestId("connections-remove-confirm-action"),
    );

    await screen.findByTestId("connections-error");
    // Nothing was removed, so nothing local is discarded either.
    expect(localStorage.getItem("ralphx-project-store:row-1")).not.toBeNull();
  });

  it("surfaces a mid-remove failure while re-listing the real staged state", async () => {
    // Rust marked pending_delete, then the host revoke leg failed and surfaced.
    listMock
      .mockResolvedValueOnce([summary()])
      .mockResolvedValue([summary({ status: "pending_delete" })]);
    removeMock.mockRejectedValue(
      new Error("REMOTE_UNREACHABLE: host unreachable"),
    );
    const user = userEvent.setup();
    render(<ConnectionsSection />);
    await screen.findByTestId("connections-row-row-1");

    await user.click(screen.getByTestId("connections-remove-row-1"));
    await user.click(
      await screen.findByTestId("connections-remove-confirm-action"),
    );

    expect(await screen.findByTestId("connections-error")).toHaveTextContent(
      /unreachable/i,
    );
    // Never a silent disappearance and never a fake success.
    await waitFor(() =>
      expect(screen.getByTestId("connections-status-row-1")).toHaveTextContent(
        "Removing…",
      ),
    );
  });
});

describe("ConnectionsSection — list failures", () => {
  it("surfaces a failed initial read instead of an empty state", async () => {
    listMock.mockRejectedValue(new Error("DATABASE_ERROR: boom"));
    render(<ConnectionsSection />);

    expect(await screen.findByTestId("connections-error")).toBeInTheDocument();
    // "No environments" is a different, much scarier claim than "could not read".
    expect(screen.queryByTestId("connections-empty")).not.toBeInTheDocument();
  });

  it("preserves an already-loaded list when a later refresh fails", async () => {
    listMock
      .mockResolvedValueOnce([summary()])
      .mockRejectedValue(new Error("DATABASE_ERROR: later"));
    const user = userEvent.setup();
    render(<ConnectionsSection />);
    await screen.findByTestId("connections-row-row-1");

    await user.click(screen.getByTestId("connections-remove-row-1"));
    await user.click(
      await screen.findByTestId("connections-remove-confirm-action"),
    );
    await screen.findByTestId("connections-error");

    expect(screen.getByTestId("connections-row-row-1")).toBeInTheDocument();
  });
});

describe("ConnectionsSection — add and re-pair", () => {
  it("opens the wizard shell synchronously, fetching nothing", async () => {
    const user = userEvent.setup();
    render(<ConnectionsSection />);
    await screen.findByTestId("connections-row-row-1");
    listMock.mockClear();

    await user.click(screen.getByTestId("connections-add"));

    expect(screen.getByTestId("add-environment-dialog")).toBeInTheDocument();
    expect(previewMock).not.toHaveBeenCalled();
    expect(listMock).not.toHaveBeenCalled();
  });

  it("prefills and locks the host when re-pairing an existing row", async () => {
    const user = userEvent.setup();
    render(<ConnectionsSection />);
    await screen.findByTestId("connections-row-row-1");

    await user.click(screen.getByTestId("connections-repair-row-1"));

    const host = screen.getByTestId("add-environment-host");
    expect(host).toHaveValue("https://studio.tail-x.ts.net:3849");
    expect(host).toBeDisabled();
    // The name field belongs to the verify step; step 1 has not been submitted yet.
    expect(
      screen.queryByTestId("add-environment-name"),
    ).not.toBeInTheDocument();
  });

  it("re-pairing a known host updates the row rather than adding a second", async () => {
    previewMock.mockResolvedValue({
      environmentId: "host-1",
      appVersion: "0.9.4",
      platform: "macOS",
      protocolVersion: 1,
      minClientProtocol: 1,
      alreadyPairedAs: "Studio Mac",
    });
    pairMock.mockResolvedValue(summary({ name: "Studio Mac" }));
    listMock.mockResolvedValue([summary({ name: "Studio Mac" })]);
    const user = userEvent.setup();
    render(<ConnectionsSection />);
    await screen.findByTestId("connections-row-row-1");

    await user.click(screen.getByTestId("connections-repair-row-1"));
    await user.type(screen.getByTestId("add-environment-code"), "rxp_ABCD1234");
    await user.click(screen.getByTestId("add-environment-continue"));
    await screen.findByTestId("add-environment-step-verify");
    await user.click(screen.getByTestId("add-environment-pair"));
    await screen.findByTestId("add-environment-success");
    await user.click(screen.getByTestId("add-environment-done"));

    await waitFor(() =>
      expect(screen.getAllByTestId(/^connections-row-/)).toHaveLength(1),
    );
  });
});
