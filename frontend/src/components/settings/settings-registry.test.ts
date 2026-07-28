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

  it("never filters any section other than the flag-gated ones", () => {
    // Stated as a set difference rather than a hardcoded count, so adding a gated
    // section updates one list instead of silently failing an arithmetic assertion.
    const gatedIds = new Set(["remote-access", "connections"]);
    const visible = visibleSettingsSections({});
    expect(visible.map((section) => section.id)).toEqual(
      SETTINGS_SECTIONS.filter((section) => !gatedIds.has(section.id)).map(
        (section) => section.id,
      ),
    );
  });
});

describe("remote-access section id", () => {
  it("is a resolvable settings section id (deep links)", () => {
    expect(isSettingsSectionId("remote-access")).toBe(true);
  });
});

describe("connections section (PR 2.5)", () => {
  it("is hidden while the remoteEnvironments flag is off", () => {
    expect(
      visibleSettingsSections({}).some((section) => section.id === "connections"),
    ).toBe(false);
    expect(
      visibleSettingsSections({ remoteEnvironments: false }).some(
        (section) => section.id === "connections",
      ),
    ).toBe(false);
  });

  it("appears beside Remote Access in the External Access group when the flag is on", () => {
    const sections = visibleSettingsSections({ remoteEnvironments: true });
    const connections = sections.find((section) => section.id === "connections");
    expect(connections).toEqual({
      id: "connections",
      groupId: "access",
      label: "Connections",
    });

    // Adjacency is the point: both surfaces of the same feature, side by side.
    const ids = sections.map((section) => section.id);
    expect(Math.abs(ids.indexOf("connections") - ids.indexOf("remote-access"))).toBe(1);
  });

  it("gates both remote sections off the same flag, not two special cases", () => {
    const gated = visibleSettingsSections({ remoteEnvironments: false });
    expect(
      gated.every(
        (section) => section.id !== "connections" && section.id !== "remote-access",
      ),
    ).toBe(true);
  });

  it("is a recognised section id", () => {
    expect(isSettingsSectionId("connections")).toBe(true);
  });
});
