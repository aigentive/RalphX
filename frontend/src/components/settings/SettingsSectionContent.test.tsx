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
  }, 15_000);
});
