import { beforeEach, describe, expect, it, vi } from "vitest";

import type { WorkspaceOpenTarget } from "@/api/chat";

import {
  PREFERRED_WORKSPACE_OPEN_TARGET_KEY,
  readPreferredWorkspaceOpenTargetId,
  resolvePreferredWorkspaceOpenTarget,
  subscribePreferredWorkspaceOpenTargetId,
  writePreferredWorkspaceOpenTargetId,
} from "./workspace-open-targets";

const targets: WorkspaceOpenTarget[] = [
  { id: "cursor", label: "Cursor", kind: "editor" },
  { id: "finder", label: "Finder", kind: "fileManager" },
];

describe("workspace-open-targets", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it("reads and writes the preferred workspace open target", () => {
    expect(readPreferredWorkspaceOpenTargetId()).toBeNull();

    writePreferredWorkspaceOpenTargetId("cursor");

    expect(readPreferredWorkspaceOpenTargetId()).toBe("cursor");
    expect(window.localStorage.getItem(PREFERRED_WORKSPACE_OPEN_TARGET_KEY)).toBe(
      "cursor",
    );
  });

  it("resolves the preferred target with first-target fallback", () => {
    expect(resolvePreferredWorkspaceOpenTarget(targets, "finder")).toEqual(
      targets[1],
    );
    expect(resolvePreferredWorkspaceOpenTarget(targets, "missing")).toEqual(
      targets[0],
    );
    expect(resolvePreferredWorkspaceOpenTarget([], "missing")).toBeNull();
  });

  it("notifies same-window and storage preference subscribers", () => {
    const listener = vi.fn();
    const unsubscribe = subscribePreferredWorkspaceOpenTargetId(listener);

    writePreferredWorkspaceOpenTargetId("finder");
    window.dispatchEvent(
      new StorageEvent("storage", {
        key: PREFERRED_WORKSPACE_OPEN_TARGET_KEY,
        newValue: "cursor",
      }),
    );

    expect(listener).toHaveBeenNthCalledWith(1, "finder");
    expect(listener).toHaveBeenNthCalledWith(2, "cursor");

    unsubscribe();
    writePreferredWorkspaceOpenTargetId("finder");
    expect(listener).toHaveBeenCalledTimes(2);
  });
});
