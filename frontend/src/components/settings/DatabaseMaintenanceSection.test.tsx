import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { DatabaseMaintenanceSection } from "./DatabaseMaintenanceSection";

function mockMaintenanceInvoke(initialPending = false) {
  let pending = initialPending;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === "get_database_maintenance_stats") {
      return {
        database_bytes: 44_530_065_408,
        reclaimable_bytes: 6_291_456,
        headroom_ok: true,
        pending_compaction: pending,
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

  it("surfaces stats load failures instead of rendering empty data", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("stats backend down"));
    render(<DatabaseMaintenanceSection />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "stats backend down",
    );
  });
});
