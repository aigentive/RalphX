import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { DatabaseMaintenanceSection } from "./DatabaseMaintenanceSection";

type MaintenanceOverrides = {
  reclaimable_bytes?: number;
  headroom_ok?: boolean;
  last_compaction?: {
    outcome: string;
    reason: string | null;
    reclaimed_bytes: number | null;
    database_bytes_before: number;
    at_rfc3339: string;
  } | null;
};

function mockMaintenanceInvoke(initialPending = false, overrides: MaintenanceOverrides = {}) {
  let pending = initialPending;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "get_database_maintenance_stats") {
      return {
        database_bytes: 44_530_065_408,
        reclaimable_bytes: overrides.reclaimable_bytes ?? 6_291_456,
        headroom_ok: overrides.headroom_ok ?? true,
        pending_compaction: pending,
        last_compaction: overrides.last_compaction ?? null,
      };
    }
    if (command === "set_database_compaction_pending") {
      pending = (args as { input: { pending: boolean } }).input.pending;
      return null;
    }
    throw new Error(`Unexpected command: ${command}`);
  });
  return () => pending;
}

describe("DatabaseMaintenanceSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders database size and reclaimable space from backend stats", async () => {
    mockMaintenanceInvoke();
    render(<DatabaseMaintenanceSection />);

    expect(await screen.findByTestId("database-size")).toHaveTextContent(
      "41 GB",
    );
    expect(screen.getByTestId("database-reclaimable")).toHaveTextContent(
      "6.0 MB",
    );
  });

  it("schedules compaction only after explicit confirmation", async () => {
    const user = userEvent.setup();
    const getPending = mockMaintenanceInvoke();
    render(<DatabaseMaintenanceSection />);

    await user.click(
      await screen.findByRole("button", { name: "Compact on next launch" }),
    );
    expect(getPending()).toBe(false);
    expect(
      screen.getByText("Compact the database on next launch?"),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "Schedule compaction" }),
    );

    await waitFor(() => expect(getPending()).toBe(true));
    expect(
      await screen.findByRole("button", { name: "Cancel scheduled compaction" }),
    ).toBeInTheDocument();
    expect(vi.mocked(invoke)).toHaveBeenCalledWith(
      "set_database_compaction_pending",
      { input: { pending: true } },
    );
  });

  it("cancels a pending compaction request", async () => {
    const user = userEvent.setup();
    const getPending = mockMaintenanceInvoke(true);
    render(<DatabaseMaintenanceSection />);

    await user.click(
      await screen.findByRole("button", {
        name: "Cancel scheduled compaction",
      }),
    );

    await waitFor(() => expect(getPending()).toBe(false));
    expect(
      await screen.findByRole("button", { name: "Compact on next launch" }),
    ).toBeInTheDocument();
  });

  it("recommends compaction only when reclaimable space is a significant share", async () => {
    mockMaintenanceInvoke(false, { reclaimable_bytes: 35_000_000_000 });
    const { unmount } = render(<DatabaseMaintenanceSection />);

    expect(await screen.findByTestId("compaction-recommended")).toBeInTheDocument();
    unmount();

    mockMaintenanceInvoke(false, { reclaimable_bytes: 6_291_456 });
    render(<DatabaseMaintenanceSection />);

    await screen.findByTestId("database-size");
    expect(screen.queryByTestId("compaction-recommended")).not.toBeInTheDocument();
  });

  it("warns before scheduling when disk headroom is insufficient", async () => {
    mockMaintenanceInvoke(false, { headroom_ok: false });
    render(<DatabaseMaintenanceSection />);

    expect(await screen.findByTestId("database-headroom-warning")).toBeInTheDocument();
  });

  it("renders the last compaction outcome including a skip reason", async () => {
    mockMaintenanceInvoke(false, {
      last_compaction: {
        outcome: "skipped",
        reason: "insufficient_disk_headroom",
        reclaimed_bytes: null,
        database_bytes_before: 44_530_065_408,
        at_rfc3339: "2026-08-10T12:00:00+00:00",
      },
    });
    render(<DatabaseMaintenanceSection />);

    expect(await screen.findByTestId("database-last-compaction")).toHaveTextContent(
      "Not enough free disk space",
    );
  });

  it("explains an interrupted swap instead of showing the raw breadcrumb reason", async () => {
    mockMaintenanceInvoke(false, {
      last_compaction: {
        outcome: "error",
        reason: "swap_interrupted",
        reclaimed_bytes: null,
        database_bytes_before: 44_530_065_408,
        at_rfc3339: "2026-08-10T12:00:00+00:00",
      },
    });
    render(<DatabaseMaintenanceSection />);

    const row = await screen.findByTestId("database-last-compaction");
    expect(row).toHaveTextContent("the original is in the backup folder");
    expect(row.textContent).not.toMatch(/swap_interrupted/);
  });

  it("describes the swap-based compaction rather than a copied backup", async () => {
    const user = userEvent.setup();
    mockMaintenanceInvoke();
    render(<DatabaseMaintenanceSection />);

    await user.click(
      await screen.findByRole("button", { name: "Compact on next launch" }),
    );

    const dialogCopy = screen.getByText(/compacts into a new file/i);
    expect(dialogCopy).toBeInTheDocument();
    expect(screen.queryByText(/verify a backup/i)).not.toBeInTheDocument();
  });

  it("keeps an accessible name on the compact button while scheduling is in flight", async () => {
    const user = userEvent.setup();
    let releaseSchedule: (() => void) | null = null;
    mockMaintenanceInvoke();
    const settled = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "set_database_compaction_pending") {
        await new Promise<void>((resolve) => { releaseSchedule = resolve; });
      }
      return settled(command, args);
    });
    render(<DatabaseMaintenanceSection />);

    await user.click(await screen.findByRole("button", { name: "Compact on next launch" }));
    await user.click(screen.getByRole("button", { name: "Schedule compaction" }));

    // Dismiss the dialog to expose the section button, which is still pending.
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    const pending = await screen.findByRole("button", { name: "Scheduling compaction" });
    expect(pending).toHaveAttribute("aria-busy", "true");
    expect(pending).toBeDisabled();

    releaseSchedule!();
    await screen.findByRole("button", { name: "Cancel scheduled compaction" });
  });

  it("surfaces stats load failures instead of rendering empty data", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("stats backend down"));
    render(<DatabaseMaintenanceSection />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "stats backend down",
    );
  });
});
