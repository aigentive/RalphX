import { render, screen, waitFor } from "@testing-library/react";

// This suite intentionally loads the real section module through the lazy
// dispatch; the first dynamic-import transform can exceed the 1s waitFor
// default, so the lazy mount gets its own generous timeout.
const LAZY_MOUNT_TIMEOUT_MS = 10_000;
import { invoke } from "@tauri-apps/api/core";
import { describe, expect, it, vi } from "vitest";

import { DEFAULT_PROJECT_SETTINGS } from "@/types/settings";

import { SettingsSectionContent } from "./SettingsSectionContent";

vi.mock("@/hooks/useIdeationSettings", () => ({
  useIdeationSettings: () => ({
    settings: null,
    updateSettings: vi.fn(),
    isLoading: false,
    isError: false,
    isUpdating: false,
    updateError: null,
  }),
}));

describe("SettingsSectionContent", () => {
  it("renders Database maintenance through its lazy live-section dispatch", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_database_maintenance_stats") {
        return {
          database_bytes: 44_530_065_408,
          reclaimable_bytes: 6_291_456,
          headroom_ok: true,
          pending_compaction: false,
          last_compaction: null,
        };
      }
      if (command === "get_data_retention_settings") {
        return {
          settings: {
            enabled: true,
            days: 90,
            archivedDays: 7,
            batchRows: 500,
            sizeBudgetBytes: null,
            sizeBudgetConfirmedAt: null,
            seededPristine: true,
            sizeBudgetAdvised: false,
            lastRunAt: null,
            lastRunPrunedRows: null,
            lastRunPayloadBytes: null,
            lastRunPayloadRows: null,
            updatedAt: "2026-08-10T12:00:00+00:00",
          },
          recommendedSizeBudgetBytes: 5_368_709_120,
        };
      }
      throw new Error(`Unexpected command: ${command}`);
    });

    render(
      <SettingsSectionContent
        section="database"
        executionSettings={DEFAULT_PROJECT_SETTINGS}
        disabled={false}
        isHydrated
        onSettingsChange={vi.fn()}
        onNavigate={vi.fn()}
        onWarmSection={vi.fn()}
      />,
    );

    const size = await screen.findByTestId(
      "database-size",
      undefined,
      { timeout: LAZY_MOUNT_TIMEOUT_MS },
    );
    await waitFor(() => expect(size).toHaveTextContent("41 GB"));
    // Retention and maintenance are one user concern, rendered in one leaf.
    expect(
      await screen.findByTestId("retention-last-run", undefined, {
        timeout: LAZY_MOUNT_TIMEOUT_MS,
      }),
    ).toBeInTheDocument();
  }, 15_000);
});
