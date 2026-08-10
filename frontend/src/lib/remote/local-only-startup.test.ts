/**
 * The startup lifecycle family must never leave this device.
 *
 * The regression this guards shipped and hard-blocked the app: `invoke` routes on the
 * GLOBAL active environment, so after a reload with a remote environment active, the
 * startup gate's own calls were sent to the host. `report_startup_frontend_milestone`
 * is unregistered there, so the shell-paint handoff rejected with
 * REMOTE_COMMAND_UNAVAILABLE, `handoffComplete` never flipped, and the client sat on
 * "STARTING RALPHX" forever — while the elapsed counter kept ticking from the LOCAL
 * backend's real start time (the tell that the subject was never the host).
 *
 * These commands describe THIS device's own boot. A host cannot answer any of them for
 * us, and `StartupRoot` scoping its QueryClient to LOCAL_ENVIRONMENT_ID does not help:
 * cache scoping and transport routing are different axes.
 */

import { describe, expect, it } from "vitest";

import { findLocalOnlyCommand } from "./local-only-commands";

const STARTUP_LIFECYCLE_COMMANDS = [
  "get_startup_status",
  "get_startup_diagnostics",
  "retry_startup",
  "report_startup_frontend_milestone",
  "open_startup_logs",
] as const;

describe("startup lifecycle commands are pinned local", () => {
  it.each(STARTUP_LIFECYCLE_COMMANDS)("%s runs locally under a remote environment", (cmd) => {
    const entry = findLocalOnlyCommand(cmd);
    expect(entry, `${cmd} must be pinned in local-only-commands.ts`).toBeDefined();
    // `reject` would fail the gate instead of answering it — the gate must still work
    // while a remote environment is active, so the only correct disposition is run-locally.
    expect(entry?.disposition).toBe("run-locally");
  });
});
