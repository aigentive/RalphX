import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { SettingsSearch } from "./SettingsSearch";
import {
  SETTINGS_SEARCH_INDEX,
  SETTINGS_SEARCH_MAX_RESULTS,
  searchSettings,
} from "./settings-search-index";
import { isSettingsSectionId } from "./settings-registry";

describe("searchSettings", () => {
  it("returns nothing for an empty query", () => {
    expect(searchSettings("")).toEqual([]);
    expect(searchSettings("   ")).toEqual([]);
  });

  it("matches labels and keywords case-insensitively", () => {
    expect(searchSettings("AUTO-MERGE").map((r) => r.section)).toContain(
      "workspace",
    );
    expect(searchSettings("jira").map((r) => r.section)).toContain(
      "integrations",
    );
  });

  it("caps the result list", () => {
    // "e" appears in nearly every entry.
    expect(searchSettings("e").length).toBeLessThanOrEqual(
      SETTINGS_SEARCH_MAX_RESULTS,
    );
  });

  it("only points at real section ids", () => {
    for (const entry of SETTINGS_SEARCH_INDEX) {
      expect(isSettingsSectionId(entry.section)).toBe(true);
    }
  });

  it("labels results with their nav breadcrumb", () => {
    const [result] = searchSettings("review policy");
    expect(result?.hint).toBe("Automation / Tasks");
  });
});

describe("SettingsSearch", () => {
  it("navigates to the destination behind a result, tab included", async () => {
    const user = userEvent.setup();
    const onNavigate = vi.fn();
    render(<SettingsSearch isOpen onNavigate={onNavigate} />);

    await user.type(screen.getByRole("searchbox"), "review policy");
    await user.click(
      screen.getByRole("option", { name: /review policy/i }),
    );

    expect(onNavigate).toHaveBeenCalledWith({
      section: "tasks",
      tab: "review-policy",
    });
  });

  it("navigates to the first result on Enter", async () => {
    const user = userEvent.setup();
    const onNavigate = vi.fn();
    render(<SettingsSearch isOpen onNavigate={onNavigate} />);

    await user.type(screen.getByRole("searchbox"), "granola{Enter}");

    expect(onNavigate).toHaveBeenCalledWith({ section: "granola" });
  });

  it("reports an empty result set instead of a silent dropdown", async () => {
    const user = userEvent.setup();
    render(<SettingsSearch isOpen onNavigate={vi.fn()} />);

    expect(screen.queryByRole("status")).not.toBeInTheDocument();

    await user.type(screen.getByRole("searchbox"), "zzzznomatch");

    expect(screen.queryAllByRole("option")).toHaveLength(0);
    expect(screen.getByRole("status")).toHaveTextContent(/no settings match/i);
  });

  it("clears the query on Escape without navigating", async () => {
    const user = userEvent.setup();
    const onNavigate = vi.fn();
    render(<SettingsSearch isOpen onNavigate={onNavigate} />);

    const box = screen.getByRole("searchbox");
    await user.type(box, "linear");
    expect(screen.queryAllByRole("option").length).toBeGreaterThan(0);

    await user.type(box, "{Escape}");
    expect(box).toHaveValue("");
    expect(screen.queryAllByRole("option")).toHaveLength(0);
    expect(onNavigate).not.toHaveBeenCalled();
  });

  it("focuses the box on Cmd-K only while the dialog is open", async () => {
    const user = userEvent.setup();
    const { rerender } = render(
      <SettingsSearch isOpen={false} onNavigate={vi.fn()} />,
    );

    await user.keyboard("{Meta>}k{/Meta}");
    expect(screen.getByRole("searchbox")).not.toHaveFocus();

    rerender(<SettingsSearch isOpen onNavigate={vi.fn()} />);
    await user.keyboard("{Meta>}k{/Meta}");
    expect(screen.getByRole("searchbox")).toHaveFocus();
  });

  it("drops the shortcut listener when the dialog closes", async () => {
    const user = userEvent.setup();
    const { rerender } = render(
      <SettingsSearch isOpen onNavigate={vi.fn()} />,
    );

    rerender(<SettingsSearch isOpen={false} onNavigate={vi.fn()} />);
    await user.keyboard("{Meta>}k{/Meta}");
    expect(screen.getByRole("searchbox")).not.toHaveFocus();
  });

  it("resets the query when the dialog closes", async () => {
    const user = userEvent.setup();
    const { rerender } = render(
      <SettingsSearch isOpen onNavigate={vi.fn()} />,
    );

    await user.type(screen.getByRole("searchbox"), "linear");
    rerender(<SettingsSearch isOpen={false} onNavigate={vi.fn()} />);

    expect(screen.getByRole("searchbox")).toHaveValue("");
  });
});
