import { describe, expect, it } from "vitest";

import {
  SETTINGS_SECTIONS,
  isSettingsSectionId,
  visibleSettingsSections,
} from "./settings-registry";

describe("visibleSettingsSections", () => {
  it("hides remote-access while the remoteEnvironments flag is off", () => {
    const withoutFlag = visibleSettingsSections({});
    expect(withoutFlag.some((section) => section.id === "remote-access")).toBe(false);
    const disabled = visibleSettingsSections({ remoteEnvironments: false });
    expect(disabled.some((section) => section.id === "remote-access")).toBe(false);
  });

  it("shows remote-access in the access group when the flag is on", () => {
    const sections = visibleSettingsSections({ remoteEnvironments: true });
    const remoteAccess = sections.find((section) => section.id === "remote-access");
    expect(remoteAccess).toBeDefined();
    expect(remoteAccess?.groupId).toBe("access");
    expect(remoteAccess?.label).toBe("Remote Access");
    expect(sections).toHaveLength(SETTINGS_SECTIONS.length);
  });

  it("never filters any other section", () => {
    const gated = visibleSettingsSections({});
    expect(gated).toHaveLength(SETTINGS_SECTIONS.length - 1);
    expect(gated.every((section) => section.id !== "remote-access")).toBe(true);
  });
});

describe("remote-access section id", () => {
  it("is a resolvable settings section id (deep links)", () => {
    expect(isSettingsSectionId("remote-access")).toBe(true);
  });
});
