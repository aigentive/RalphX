import { describe, expect, it } from "vitest";

import type { ClickUpIntegrationSettings } from "@/api/clickup";

import { clickupIntegrationKeys, isClickUpConnected } from "./useClickUpIntegration";

function settings(
  overrides: Partial<ClickUpIntegrationSettings> = {},
): ClickUpIntegrationSettings {
  return {
    enabled: true,
    hasApiToken: true,
    workspaceId: "team-1",
    validationStatus: "valid",
    taskSearchAvailable: true,
    lastValidatedAt: null,
    lastError: null,
    updatedAt: new Date(0).toISOString(),
    ...overrides,
  };
}

describe("isClickUpConnected", () => {
  it("is true only when enabled, token stored, valid, and task search available", () => {
    expect(isClickUpConnected(settings())).toBe(true);
  });

  it("is false when any gate is unmet", () => {
    expect(isClickUpConnected(undefined)).toBe(false);
    expect(isClickUpConnected(settings({ enabled: false }))).toBe(false);
    expect(isClickUpConnected(settings({ hasApiToken: false }))).toBe(false);
    expect(isClickUpConnected(settings({ validationStatus: "invalid" }))).toBe(
      false,
    );
    expect(isClickUpConnected(settings({ taskSearchAvailable: false }))).toBe(
      false,
    );
  });
});

describe("clickupIntegrationKeys", () => {
  it("namespaces settings and workspaces query keys", () => {
    expect(clickupIntegrationKeys.settings()).toEqual([
      "clickup-integration",
      "settings",
    ]);
    expect(clickupIntegrationKeys.workspaces()).toEqual([
      "clickup-integration",
      "workspaces",
    ]);
  });
});
