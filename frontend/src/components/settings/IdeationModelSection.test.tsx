import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { IdeationModelResponse } from "@/api/ideation-model";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import { useIdeationModelSettings } from "@/hooks/useIdeationModelSettings";
import { useProjectStore } from "@/stores/projectStore";

import { IdeationModelSection } from "./IdeationModelSection";

vi.mock("@/hooks/useHarnessProviders", () => ({
  useHarnessProviders: vi.fn(),
}));

vi.mock("@/hooks/useIdeationModelSettings", () => ({
  useIdeationModelSettings: vi.fn(),
}));

vi.mock("@/stores/projectStore", () => ({
  selectActiveProject: vi.fn(),
  useProjectStore: vi.fn(),
}));

const globalUpdateSettings = vi.fn();
const projectUpdateSettings = vi.fn();

if (!HTMLElement.prototype.scrollIntoView) {
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    value: vi.fn(),
    writable: true,
  });
}

const modelSettings: IdeationModelResponse = {
  primaryModel: "inherit",
  verifierModel: "inherit",
  verifierSubagentModel: "inherit",
  effectivePrimaryModel: "sonnet",
  effectiveVerifierModel: "sonnet",
  effectiveVerifierSubagentModel: "sonnet",
  primaryModelSource: "default",
  verifierModelSource: "default",
  verifierSubagentModelSource: "default",
  ideationSubagentModel: "inherit",
  effectiveIdeationSubagentModel: "sonnet",
  ideationSubagentModelSource: "default",
};

function openSelect(id: string) {
  const trigger = document.getElementById(id);
  expect(trigger).not.toBeNull();
  fireEvent.keyDown(trigger!, { key: "ArrowDown", code: "ArrowDown" });
}

describe("IdeationModelSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(useHarnessProviders).mockReturnValue({
      providers: [
        {
          provider: "claude",
          supportedModelAliases: ["fable", "claude-opus-4-7", "opus", "sonnet", "haiku"],
        },
      ],
    } as ReturnType<typeof useHarnessProviders>);
    vi.mocked(useProjectStore).mockReturnValue({ id: "project-1", name: "Project One" });
    vi.mocked(useIdeationModelSettings).mockImplementation((projectId) => ({
      settings: modelSettings,
      isPlaceholderData: false,
      updateSettings: projectId === null ? globalUpdateSettings : projectUpdateSettings,
      saveError: null,
    }) as ReturnType<typeof useIdeationModelSettings>);
  });

  it("uses the same capability-derived availability in every global and project row", () => {
    render(<IdeationModelSection />);

    for (const id of [
      "global-primary-model",
      "ideation-subagent-model",
      "project-primary-model",
      "project-ideation-subagent-model",
    ]) {
      openSelect(id);
      expect(screen.getByRole("option", { name: /Claude Opus 4\.7/ })).not.toHaveAttribute(
        "data-disabled",
      );
      expect(screen.getByRole("option", { name: /Claude Opus 4\.8/ })).toHaveAttribute(
        "aria-disabled",
        "true",
      );
      expect(screen.getByRole("option", { name: /Claude Opus 5/ })).toHaveAttribute(
        "aria-disabled",
        "true",
      );
      expect(screen.getByText("Claude Opus 4.8, requires Claude Code 2.1.154+")).toBeInTheDocument();
      fireEvent.keyDown(document.activeElement!, { key: "Escape", code: "Escape" });
    }
  });

  it("shows Claude model options in canonical picker order", () => {
    vi.mocked(useHarnessProviders).mockReturnValue({
      providers: [
        {
          provider: "claude",
          supportedModelAliases: [
            "fable",
            "claude-opus-5",
            "claude-opus-4-8",
            "claude-opus-4-7",
            "opus",
            "sonnet",
            "haiku",
          ],
        },
      ],
    } as ReturnType<typeof useHarnessProviders>);
    render(<IdeationModelSection />);

    openSelect("global-primary-model");

    const optionLabels = [
      "Inherit",
      "Fable",
      "Claude Opus 5",
      "Claude Opus 4.8",
      "Claude Opus 4.7",
      "Opus",
      "Sonnet",
      "Haiku",
    ];
    const options = screen.getAllByRole("option");

    expect(options).toHaveLength(optionLabels.length);
    optionLabels.forEach((label, index) => {
      expect(within(options[index]!).getByText(label)).toBeInTheDocument();
    });
  });

  it("persists supported exact selections and rejects disabled exact choices", async () => {
    const user = userEvent.setup();
    vi.mocked(useHarnessProviders).mockReturnValue({
      providers: [
        {
          provider: "claude",
          supportedModelAliases: [
            "sonnet",
            "opus",
            "haiku",
            "fable",
            "claude-opus-4-7",
            "claude-opus-4-8",
            "claude-opus-5",
          ],
        },
      ],
    } as ReturnType<typeof useHarnessProviders>);
    const view = render(<IdeationModelSection />);

    openSelect("global-primary-model");
    await user.click(screen.getByRole("option", { name: /Claude Opus 4\.7/ }));
    expect(globalUpdateSettings).toHaveBeenCalledWith(
      { primaryModel: "claude-opus-4-7" },
      expect.any(Object),
    );

    openSelect("ideation-subagent-model");
    await user.click(screen.getByRole("option", { name: /Claude Opus 4\.8/ }));
    expect(globalUpdateSettings).toHaveBeenCalledWith(
      { ideationSubagentModel: "claude-opus-4-8" },
      expect.any(Object),
    );

    openSelect("project-primary-model");
    await user.click(screen.getByRole("option", { name: /Claude Opus 5/ }));
    expect(projectUpdateSettings).toHaveBeenCalledWith(
      { primaryModel: "claude-opus-5" },
      expect.any(Object),
    );

    openSelect("project-ideation-subagent-model");
    await user.click(screen.getByRole("option", { name: /Claude Opus 4\.7/ }));
    expect(projectUpdateSettings).toHaveBeenCalledWith(
      { ideationSubagentModel: "claude-opus-4-7" },
      expect.any(Object),
    );

    vi.mocked(useHarnessProviders).mockReturnValue({
      providers: [
        {
          provider: "claude",
          supportedModelAliases: ["fable", "claude-opus-4-7", "opus", "sonnet", "haiku"],
        },
      ],
    } as ReturnType<typeof useHarnessProviders>);
    view.unmount();
    render(<IdeationModelSection />);
    globalUpdateSettings.mockClear();
    openSelect("global-primary-model");
    await user.click(screen.getByRole("option", { name: /Claude Opus 4\.8/ }));
    expect(globalUpdateSettings).not.toHaveBeenCalled();
  });
});
