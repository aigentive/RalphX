/**
 * The dock badge is painted on the machine the command runs on.
 *
 * `set_dock_badge_count` calls `set_macos_dock_badge` on the running process's main thread
 * (`commands/notification_commands.rs`), so routing it to the host badges the HOST's dock
 * with this client's count — and this Mac's own dock icon never updates. Same class as the
 * startup lifecycle bug: a device-local ACTION sent to the wrong machine.
 *
 * The rest of the notification surface is the opposite: the count, the list, the attention
 * items, and the read-marks all describe the host's workspace and are correctly registered on
 * the facade. Only the act of painting a dock icon is local.
 */

import { describe, expect, it } from "vitest";

import { findLocalOnlyCommand } from "./local-only-commands";

describe("dock badge is painted locally, counted remotely", () => {
  it("pins set_dock_badge_count to the local transport", () => {
    const entry = findLocalOnlyCommand("set_dock_badge_count");
    expect(entry, "set_dock_badge_count must be pinned in local-only-commands.ts").toBeDefined();
    // `run-locally`, not `reject`: the badge must still be painted while a remote environment
    // is active — it is showing the host's count on this Mac's dock, which is the point.
    expect(entry?.disposition).toBe("run-locally");
  });

  it.each([
    "get_unread_notification_count",
    "list_notifications",
    "list_attention_items",
    "mark_notification_read",
    "mark_all_notifications_read",
  ])("leaves %s host-served", (cmd) => {
    // These describe the ACTIVE environment's workspace. Pinning them local would show this
    // Mac's notifications while connected to another machine.
    expect(findLocalOnlyCommand(cmd)).toBeUndefined();
  });
});
