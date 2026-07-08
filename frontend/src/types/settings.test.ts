// Settings types tests

import { describe, it, expect } from "vitest";
import {
  ExecutionSettingsSchema,
  ProjectReviewSettingsSchema,
  ProjectSettingsSchema,
  SettingsProfileSchema,
  DEFAULT_EXECUTION_SETTINGS,
  DEFAULT_PROJECT_REVIEW_SETTINGS,
  DEFAULT_PROJECT_SETTINGS,
  parseProjectSettings,
  safeParseProjectSettings,
  parseSettingsProfile,
  safeParseSettingsProfile,
} from "./settings";

describe("ExecutionSettingsSchema", () => {
  it("parses valid execution settings", () => {
    const data = {
      max_concurrent_tasks: 4,
      project_ideation_max: 3,
      auto_commit: false,
      commit_message_prefix: "fix: ",
      pause_on_failure: false,
      agent_workspace_pr_autofix_default: true,
      agent_workspace_pr_auto_merge_default: true,
    };
    const result = ExecutionSettingsSchema.parse(data);
    expect(result.max_concurrent_tasks).toBe(4);
    expect(result.project_ideation_max).toBe(3);
    expect(result.auto_commit).toBe(false);
    expect(result.commit_message_prefix).toBe("fix: ");
    expect(result.agent_workspace_pr_autofix_default).toBe(true);
    expect(result.agent_workspace_pr_auto_merge_default).toBe(true);
  });

  it("applies defaults for missing fields", () => {
    const result = ExecutionSettingsSchema.parse({});
    expect(result).toEqual(DEFAULT_EXECUTION_SETTINGS);
  });

  it("validates max_concurrent_tasks range", () => {
    expect(() =>
      ExecutionSettingsSchema.parse({ max_concurrent_tasks: 0 }),
    ).toThrow();
    expect(() =>
      ExecutionSettingsSchema.parse({ max_concurrent_tasks: 11 }),
    ).toThrow();
    expect(
      ExecutionSettingsSchema.parse({ max_concurrent_tasks: 5 })
        .max_concurrent_tasks,
    ).toBe(5);
  });

  it("allows disabling project ideation with a zero cap", () => {
    const result = ExecutionSettingsSchema.parse({ project_ideation_max: 0 });
    expect(result.project_ideation_max).toBe(0);
  });
});

describe("ProjectReviewSettingsSchema", () => {
  it("parses valid review settings", () => {
    const data = {
      ai_review_enabled: false,
      ai_review_auto_fix: false,
      require_fix_approval: true,
      require_human_review: true,
      require_workspace_review: false,
      autofix_workspace_review_blocking_findings: false,
      max_fix_attempts: 5,
    };
    const result = ProjectReviewSettingsSchema.parse(data);
    expect(result.ai_review_enabled).toBe(false);
    expect(result.require_human_review).toBe(true);
    expect(result.require_workspace_review).toBe(false);
    expect(result.autofix_workspace_review_blocking_findings).toBe(false);
    expect(result.max_fix_attempts).toBe(5);
  });

  it("applies defaults for missing fields", () => {
    const result = ProjectReviewSettingsSchema.parse({});
    expect(result).toEqual(DEFAULT_PROJECT_REVIEW_SETTINGS);
  });

  it("validates max_fix_attempts range", () => {
    expect(() =>
      ProjectReviewSettingsSchema.parse({ max_fix_attempts: 0 }),
    ).toThrow();
    expect(() =>
      ProjectReviewSettingsSchema.parse({ max_fix_attempts: 11 }),
    ).toThrow();
    expect(
      ProjectReviewSettingsSchema.parse({ max_fix_attempts: 7 })
        .max_fix_attempts,
    ).toBe(7);
  });
});

describe("ProjectSettingsSchema", () => {
  it("parses full project settings", () => {
    const data = {
      execution: { max_concurrent_tasks: 3 },
      review: { ai_review_enabled: false },
    };
    const result = ProjectSettingsSchema.parse(data);
    expect(result.execution.max_concurrent_tasks).toBe(3);
    expect(result.review.ai_review_enabled).toBe(false);
  });

  it("applies all defaults for empty object", () => {
    const result = ProjectSettingsSchema.parse({});
    expect(result).toEqual(DEFAULT_PROJECT_SETTINGS);
  });

  it("applies partial defaults", () => {
    const result = ProjectSettingsSchema.parse({
      execution: { max_concurrent_tasks: 4 },
    });
    expect(result.execution.max_concurrent_tasks).toBe(4);
    expect(result.execution.auto_commit).toBe(true); // default
    expect(result.review).toEqual(DEFAULT_PROJECT_REVIEW_SETTINGS);
  });
});

describe("SettingsProfileSchema", () => {
  it("parses valid settings profile", () => {
    const data = {
      id: "profile-1",
      name: "Development",
      description: "Settings for development",
      settings: DEFAULT_PROJECT_SETTINGS,
      isDefault: true,
      createdAt: "2026-01-24T10:00:00Z",
      updatedAt: "2026-01-24T11:00:00Z",
    };
    const result = SettingsProfileSchema.parse(data);
    expect(result.id).toBe("profile-1");
    expect(result.name).toBe("Development");
    expect(result.isDefault).toBe(true);
  });

  it("requires id and name", () => {
    expect(() =>
      SettingsProfileSchema.parse({
        settings: DEFAULT_PROJECT_SETTINGS,
        createdAt: "2026-01-24T10:00:00Z",
        updatedAt: "2026-01-24T10:00:00Z",
      }),
    ).toThrow();
  });
});

describe("parseProjectSettings", () => {
  it("parses valid data", () => {
    const result = parseProjectSettings({});
    expect(result).toEqual(DEFAULT_PROJECT_SETTINGS);
  });

  it("throws on invalid data", () => {
    expect(() =>
      parseProjectSettings({
        execution: { max_concurrent_tasks: "invalid" },
      }),
    ).toThrow();
  });
});

describe("safeParseProjectSettings", () => {
  it("returns settings for valid data", () => {
    const result = safeParseProjectSettings({});
    expect(result).toEqual(DEFAULT_PROJECT_SETTINGS);
  });

  it("returns null for invalid data", () => {
    const result = safeParseProjectSettings({
      execution: { max_concurrent_tasks: "invalid" },
    });
    expect(result).toBeNull();
  });
});

describe("parseSettingsProfile", () => {
  it("parses valid data", () => {
    const result = parseSettingsProfile({
      id: "p1",
      name: "Test",
      settings: {},
      createdAt: "2026-01-24T10:00:00Z",
      updatedAt: "2026-01-24T10:00:00Z",
    });
    expect(result.id).toBe("p1");
    expect(result.settings).toEqual(DEFAULT_PROJECT_SETTINGS);
  });

  it("throws on invalid data", () => {
    expect(() => parseSettingsProfile({})).toThrow();
  });
});

describe("safeParseSettingsProfile", () => {
  it("returns profile for valid data", () => {
    const result = safeParseSettingsProfile({
      id: "p1",
      name: "Test",
      settings: {},
      createdAt: "2026-01-24T10:00:00Z",
      updatedAt: "2026-01-24T10:00:00Z",
    });
    expect(result).not.toBeNull();
    expect(result?.id).toBe("p1");
  });

  it("returns null for invalid data", () => {
    const result = safeParseSettingsProfile({});
    expect(result).toBeNull();
  });
});
