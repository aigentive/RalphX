import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import {
  resetSkillsEnabledForTests,
  setSkillsEnabled,
  useSkillsEnabled,
} from "./skillsSettingsStore";

describe("skillsSettingsStore", () => {
  beforeEach(() => {
    resetSkillsEnabledForTests(true);
    window.localStorage.clear();
  });

  it("persists skills visibility changes and ignores unchanged writes", () => {
    const { result } = renderHook(() => useSkillsEnabled());

    expect(result.current[0]).toBe(true);

    act(() => {
      result.current[1](false);
    });

    expect(result.current[0]).toBe(false);
    expect(window.localStorage.getItem("ralphx-skills-enabled")).toBe("false");

    act(() => {
      setSkillsEnabled(false);
    });

    expect(result.current[0]).toBe(false);

    act(() => {
      result.current[1](true);
    });

    expect(result.current[0]).toBe(true);
    expect(window.localStorage.getItem("ralphx-skills-enabled")).toBe("true");
  });

  it("resets the in-memory setting and clears persisted test state", () => {
    const { result } = renderHook(() => useSkillsEnabled());

    act(() => {
      setSkillsEnabled(false);
    });
    expect(result.current[0]).toBe(false);

    act(() => {
      resetSkillsEnabledForTests(true);
    });

    expect(result.current[0]).toBe(true);
    expect(window.localStorage.getItem("ralphx-skills-enabled")).toBeNull();
  });
});
