import { describe, expect, it } from "vitest";

import {
  AppViewSchema,
  parsePersistedViewByProject,
} from "./app-view";

describe("AppView", () => {
  it("accepts only live root views", () => {
    expect(AppViewSchema.parse("agents")).toBe("agents");
    expect(AppViewSchema.safeParse("graph").success).toBe(false);
  });

  it("normalizes legacy and malformed persisted routes to Agents", () => {
    const result = parsePersistedViewByProject({
      projectA: "graph",
      projectB: "agents",
      projectC: "settings",
      projectD: "not-a-view",
      "": "agents",
      projectE: 42,
    });

    expect(result).toEqual({
      map: {
        projectA: "agents",
        projectB: "agents",
        projectC: "agents",
        projectD: "agents",
        projectE: "agents",
      },
      changed: true,
    });
  });

  it("rejects non-object persisted values without inventing project keys", () => {
    expect(parsePersistedViewByProject(null)).toEqual({ map: {}, changed: true });
    expect(parsePersistedViewByProject(["agents"])).toEqual({ map: {}, changed: true });
  });

  it("reports unchanged data so storage is rewritten only once", () => {
    expect(
      parsePersistedViewByProject({ projectA: "agents", projectB: "activity" }),
    ).toEqual({
      map: { projectA: "agents", projectB: "activity" },
      changed: false,
    });
  });
});
