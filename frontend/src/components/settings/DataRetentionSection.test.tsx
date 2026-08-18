import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { DataRetentionSection } from "./DataRetentionSection";

const GIB = 1024 * 1024 * 1024;

type SettingsOverrides = {
  sizeBudgetBytes?: number | null;
  sizeBudgetConfirmedAt?: string | null;
  sizeBudgetAdvised?: boolean;
  lastRunAt?: string | null;
  lastRunPayloadBytes?: number | null;
  lastRunPrunedRows?: number | null;
};

function settingsResponse(overrides: SettingsOverrides = {}) {
  return {
    settings: {
      enabled: true,
      days: 90,
      archivedDays: 7,
      batchRows: 500,
      sizeBudgetBytes: overrides.sizeBudgetBytes ?? null,
      sizeBudgetConfirmedAt: overrides.sizeBudgetConfirmedAt ?? null,
      seededPristine: true,
      sizeBudgetAdvised: overrides.sizeBudgetAdvised ?? false,
      lastRunAt: overrides.lastRunAt ?? null,
      lastRunPrunedRows: overrides.lastRunPrunedRows ?? null,
      lastRunPayloadBytes: overrides.lastRunPayloadBytes ?? null,
      lastRunPayloadRows: null,
      updatedAt: "2026-08-10T12:00:00+00:00",
    },
    recommendedSizeBudgetBytes: 5 * GIB,
  };
}

type RetentionCalls = { command: string; args: unknown }[];

function mockRetentionInvoke(overrides: SettingsOverrides = {}) {
  const calls: RetentionCalls = [];
  let current = settingsResponse(overrides);
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    calls.push({ command, args });
    if (command === "get_data_retention_settings") return current;
    if (command === "update_data_retention_settings") {
      const input = (args as { input: Record<string, unknown> }).input;
      current = {
        ...current,
        settings: {
          ...current.settings,
          ...input,
          sizeBudgetBytes: (input.sizeBudgetBytes as number | null) ?? null,
          sizeBudgetConfirmedAt: input.sizeBudgetConfirmed && input.sizeBudgetBytes
            ? "2026-08-11T00:00:00+00:00"
            : null,
        },
      };
      return current;
    }
    if (command === "preview_data_retention_size_budget") {
      return { rows: 873_000, bytes: 30 * GIB, cutCreatedAt: "2026-06-01T00:00:00+00:00" };
    }
    if (command === "run_data_retention_now") {
      return {
        prunedRows: 12,
        payloadBytesAfter: 1024,
        payloadRowsAfter: 3,
        databaseBytesAfter: 44_530_065_408,
        reclaimableHintBytes: 35_000_000_000,
        compactionRecommended: true,
        sizeBudgetAdvised: false,
        sizeBudgetActive: false,
        skippedReason: null,
      };
    }
    throw new Error(`Unexpected command: ${command}`);
  });
  return calls;
}

const updates = (calls: RetentionCalls) =>
  calls.filter((call) => call.command === "update_data_retention_settings");

describe("DataRetentionSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders the shipped time-window policy with the size limit off", async () => {
    mockRetentionInvoke();
    render(<DataRetentionSection />);

    expect(await screen.findByLabelText("Keep tool-call detail for")).toHaveValue(90);
    expect(screen.getByLabelText("Archived conversations")).toHaveValue(7);
    expect(
      screen.getByRole("button", { name: "Enable size limit…" }),
    ).toBeInTheDocument();
    expect(screen.getByText(/Off\. RalphX keeps every tool-call detail/)).toBeInTheDocument();
  });

  it("saves a changed retention window through the update command", async () => {
    const user = userEvent.setup();
    const calls = mockRetentionInvoke();
    render(<DataRetentionSection />);

    const days = await screen.findByLabelText("Keep tool-call detail for");
    // The input renders (disabled) before settings finish loading, so wait for
    // it to become interactive before clearing — otherwise this races the load.
    await waitFor(() => expect(days).toBeEnabled());
    await user.clear(days);
    await user.type(days, "30");
    await user.tab();

    await waitFor(() => expect(updates(calls)).toHaveLength(1));
    expect((updates(calls)[0]?.args as { input: { days: number } }).input.days).toBe(30);
  });

  it("does not enable the size budget until the confirm dialog is accepted", async () => {
    const user = userEvent.setup();
    const calls = mockRetentionInvoke();
    render(<DataRetentionSection />);

    await user.click(await screen.findByRole("button", { name: "Enable size limit…" }));

    // Preview runs first; nothing is persisted yet.
    await screen.findByTestId("size-budget-preview");
    expect(updates(calls)).toHaveLength(0);
    expect(screen.getByTestId("size-budget-preview-rows")).toHaveTextContent("873,000");
    expect(screen.getByTestId("size-budget-preview-bytes")).toHaveTextContent("30 GB");

    await user.click(screen.getByRole("button", { name: "Enable size limit" }));

    await waitFor(() => expect(updates(calls)).toHaveLength(1));
    const input = (updates(calls)[0]?.args as {
      input: { sizeBudgetBytes: number; sizeBudgetConfirmed: boolean };
    }).input;
    expect(input.sizeBudgetBytes).toBe(5 * GIB);
    expect(input.sizeBudgetConfirmed).toBe(true);
  });

  it("leaves the stored policy untouched when the confirmation is cancelled", async () => {
    const user = userEvent.setup();
    const calls = mockRetentionInvoke();
    render(<DataRetentionSection />);

    await user.click(await screen.findByRole("button", { name: "Enable size limit…" }));
    await screen.findByTestId("size-budget-preview");
    await user.click(screen.getByRole("button", { name: "Cancel" }));

    await waitFor(() =>
      expect(screen.queryByTestId("size-budget-preview")).not.toBeInTheDocument(),
    );
    expect(updates(calls)).toHaveLength(0);
  });

  it("previews again when lowering a budget but not when raising it", async () => {
    const user = userEvent.setup();
    const calls = mockRetentionInvoke({
      sizeBudgetBytes: 5 * GIB,
      sizeBudgetConfirmedAt: "2026-08-01T00:00:00+00:00",
    });
    render(<DataRetentionSection />);

    const budget = await screen.findByLabelText("Size limit in GB");
    await user.clear(budget);
    await user.type(budget, "10");
    await user.click(screen.getByRole("button", { name: "Update limit" }));

    await waitFor(() => expect(updates(calls)).toHaveLength(1));
    expect(
      calls.some((call) => call.command === "preview_data_retention_size_budget"),
    ).toBe(false);

    await user.clear(budget);
    await user.type(budget, "2");
    await user.click(screen.getByRole("button", { name: "Update limit" }));

    await screen.findByTestId("size-budget-preview");
    expect(updates(calls)).toHaveLength(1);
  });

  it("turning the budget off clears it without a preview", async () => {
    const user = userEvent.setup();
    const calls = mockRetentionInvoke({
      sizeBudgetBytes: 5 * GIB,
      sizeBudgetConfirmedAt: "2026-08-01T00:00:00+00:00",
    });
    render(<DataRetentionSection />);

    await user.click(await screen.findByRole("button", { name: "Turn off" }));

    await waitFor(() => expect(updates(calls)).toHaveLength(1));
    const input = (updates(calls)[0]?.args as {
      input: { sizeBudgetBytes: number | null; sizeBudgetConfirmed: boolean };
    }).input;
    expect(input.sizeBudgetBytes).toBeNull();
    expect(input.sizeBudgetConfirmed).toBe(false);
  });

  it("renders the persisted advisory on open and hides it once a budget is active", async () => {
    mockRetentionInvoke({ sizeBudgetAdvised: true, lastRunPayloadBytes: 35 * GIB });
    const { unmount } = render(<DataRetentionSection />);

    expect(await screen.findByTestId("size-budget-advisory")).toHaveTextContent("35 GB");
    unmount();

    mockRetentionInvoke({
      sizeBudgetAdvised: true,
      sizeBudgetBytes: 5 * GIB,
      sizeBudgetConfirmedAt: "2026-08-01T00:00:00+00:00",
    });
    render(<DataRetentionSection />);

    await screen.findByLabelText("Keep tool-call detail for");
    expect(screen.queryByTestId("size-budget-advisory")).not.toBeInTheDocument();
  });

  it("describes run-now truthfully for a time-window-only policy", async () => {
    const user = userEvent.setup();
    mockRetentionInvoke();
    render(<DataRetentionSection />);

    await user.click(await screen.findByRole("button", { name: "Run cleanup now" }));

    expect(
      screen.getByText(/Nothing inside the window is touched/),
    ).toBeInTheDocument();
  });

  it("describes run-now differently once the size limit is active", async () => {
    const user = userEvent.setup();
    mockRetentionInvoke({
      sizeBudgetBytes: 5 * GIB,
      sizeBudgetConfirmedAt: "2026-08-01T00:00:00+00:00",
    });
    render(<DataRetentionSection />);

    await user.click(await screen.findByRole("button", { name: "Run cleanup now" }));

    expect(
      screen.getByText(/any oldest detail above the size limit/),
    ).toBeInTheDocument();
  });

  it("reports the cycle result and recommends compaction when space is only logically freed", async () => {
    const user = userEvent.setup();
    mockRetentionInvoke();
    render(<DataRetentionSection />);

    await user.click(await screen.findByRole("button", { name: "Run cleanup now" }));
    await user.click(screen.getByRole("button", { name: "Run cleanup" }));

    const report = await screen.findByTestId("retention-run-report");
    expect(report).toHaveTextContent("Removed 12 records");
    expect(report).toHaveTextContent(/schedule a compaction/i);
  });

  it("renders last-run measurements without scanning on open", async () => {
    const calls = mockRetentionInvoke({
      lastRunAt: "2026-08-10T12:00:00+00:00",
      lastRunPrunedRows: 4_200,
      lastRunPayloadBytes: 2 * GIB,
    });
    render(<DataRetentionSection />);

    expect(await screen.findByTestId("retention-last-run")).toHaveTextContent("4,200 records removed");
    expect(calls.map((call) => call.command)).toEqual(["get_data_retention_settings"]);
  });

  it("shows removed-record count but omits size when lastRunPayloadBytes is null", async () => {
    mockRetentionInvoke({
      lastRunAt: "2026-08-10T12:00:00+00:00",
      lastRunPrunedRows: 99,
      lastRunPayloadBytes: null,
    });
    render(<DataRetentionSection />);

    const el = await screen.findByTestId("retention-last-run");
    expect(el).toHaveTextContent("99 records removed");
    expect(el.textContent).not.toMatch(/0 B/);
    expect(el.textContent).not.toMatch(/stored/);
  });

  it("keeps an accessible name on the size-limit button while the preview is in flight", async () => {
    const user = userEvent.setup();
    let releasePreview: (() => void) | null = null;
    mockRetentionInvoke();
    const settled = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "preview_data_retention_size_budget") {
        await new Promise<void>((resolve) => { releasePreview = resolve; });
      }
      return settled(command, args);
    });
    render(<DataRetentionSection />);

    await user.click(await screen.findByRole("button", { name: "Enable size limit…" }));

    const pending = await screen.findByRole("button", { name: "Checking the size limit" });
    expect(pending).toHaveAttribute("aria-busy", "true");

    releasePreview!();
    await screen.findByTestId("size-budget-preview");
  });

  it("keeps an accessible name on the run-cleanup button while the cycle is in flight", async () => {
    const user = userEvent.setup();
    let releaseRun: (() => void) | null = null;
    mockRetentionInvoke();
    const settled = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "run_data_retention_now") {
        await new Promise<void>((resolve) => { releaseRun = resolve; });
      }
      return settled(command, args);
    });
    render(<DataRetentionSection />);

    await user.click(await screen.findByRole("button", { name: "Run cleanup now" }));
    await user.click(await screen.findByRole("button", { name: "Run cleanup" }));

    // The dialog stays open for the whole cycle, so its action is the reachable control.
    const inDialog = await screen.findByRole("button", { name: "Running cleanup" });
    expect(inDialog).toHaveAttribute("aria-busy", "true");
    expect(inDialog).toBeDisabled();

    // Dismissing mid-run uncovers the section button, which is still pending.
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    const inSection = await screen.findByRole("button", { name: "Running cleanup" });
    expect(inSection).toHaveAttribute("aria-busy", "true");

    releaseRun!();
    await screen.findByTestId("retention-run-report");
  });

  it("surfaces load failures instead of rendering empty policy values", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("retention backend down"));
    render(<DataRetentionSection />);

    expect(await screen.findByRole("alert")).toHaveTextContent("retention backend down");
  });
});
