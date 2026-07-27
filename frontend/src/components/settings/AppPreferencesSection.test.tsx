import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AppPreferencesSection } from "./AppPreferencesSection";
import { resetSkillsEnabledForTests } from "@/stores/skillsSettingsStore";

const setCurrentViewMock = vi.fn();

let currentView = "agents";

vi.mock("@/stores/uiStore", () => ({
  useUiStore: (selector: (state: {
    currentView: string;
    setCurrentView: typeof setCurrentViewMock;
  }) => unknown) =>
    selector({
      currentView,
      setCurrentView: setCurrentViewMock,
    }),
}));

describe("AppPreferencesSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.localStorage.clear();
    currentView = "agents";
    resetSkillsEnabledForTests(true);
  });

  it("persists the Skills surface toggle", async () => {
    render(<AppPreferencesSection />);

    fireEvent.click(screen.getByTestId("skills-surface-enabled"));

    await waitFor(() => {
      expect(window.localStorage.getItem("ralphx-skills-enabled")).toBe("false");
    });
  });

  it("returns to Agents when Skills is disabled from the Skills view", async () => {
    currentView = "skills";
    render(<AppPreferencesSection />);

    fireEvent.click(screen.getByTestId("skills-surface-enabled"));

    await waitFor(() => {
      expect(setCurrentViewMock).toHaveBeenCalledWith("agents");
    });
  });
});
