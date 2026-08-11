import React from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ReleaseNotesDialog } from "./ReleaseNotesDialog";
import {
  ReleaseMetadataSnapshot,
  type ReleaseMetadata,
} from "@/api/release-notes";
import type { UpdateChannel } from "@/api/update-channel";

const mocks = vi.hoisted(() => ({
  listReleaseNotesVersions: vi.fn(),
  fetchReleaseMetadata: vi.fn(),
  getReleaseNotesForVersion: vi.fn(),
  getVersion: vi.fn(),
  compareSemverDesc: vi.fn(),
  mergeVersionLists: vi.fn(),
  updateChannel: "stable" as UpdateChannel,
  isUpdateChannelSettled: true,
  isUpdateChannelLoading: false,
  isUpdateChannelSaving: false,
  updateChannelLoadError: null as Error | null,
  updateChannelSaveError: null as Error | null,
  setUpdateChannel: vi.fn(),
}));

vi.mock("@/api/release-notes", async () => {
  const actual = await vi.importActual<Record<string, unknown>>("@/api/release-notes");
  return {
    ...actual,
    listReleaseNotesVersions: (...args: unknown[]) => mocks.listReleaseNotesVersions(...args),
    fetchReleaseMetadata: (...args: unknown[]) => mocks.fetchReleaseMetadata(...args),
    getReleaseNotesForVersion: (...args: unknown[]) => mocks.getReleaseNotesForVersion(...args),
  };
});

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: (...args: unknown[]) => mocks.getVersion(...args),
}));

vi.mock("@/hooks/useUpdateChannel", () => ({
  useUpdateChannel: () => ({
    updateChannel: mocks.updateChannel,
    isSettled: mocks.isUpdateChannelSettled,
    isLoading: mocks.isUpdateChannelLoading,
    isError: mocks.updateChannelLoadError !== null,
    loadError: mocks.updateChannelLoadError,
    setUpdateChannel: mocks.setUpdateChannel,
    isSaving: mocks.isUpdateChannelSaving,
    saveError: mocks.updateChannelSaveError,
  }),
}));

vi.mock("react-markdown", () => ({
  default: ({ children }: { children: string }) =>
    React.createElement("div", { "data-testid": "markdown" }, children),
}));

vi.mock("remark-gfm", () => ({
  default: () => {},
}));

vi.mock("@/components/Chat/MessageItem.markdown", () => ({
  markdownComponents: {},
}));

function makeMetadata(
  entries: Array<{
    version: string;
    publishedAt: string;
    body?: string | null;
    prerelease?: boolean;
  }>,
): Map<string, ReleaseMetadata> {
  const map = new Map<string, ReleaseMetadata>();
  for (const e of entries) {
    map.set(e.version, {
      version: e.version,
      publishedAt: e.publishedAt,
      name: `v${e.version}`,
      body: e.body ?? null,
      prerelease: e.prerelease ?? false,
    });
  }
  return map;
}

async function flushAll() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("ReleaseNotesDialog", () => {
  beforeEach(() => {
    mocks.updateChannel = "stable";
    mocks.isUpdateChannelSettled = true;
    mocks.isUpdateChannelLoading = false;
    mocks.isUpdateChannelSaving = false;
    mocks.updateChannelLoadError = null;
    mocks.updateChannelSaveError = null;
    mocks.setUpdateChannel.mockReset();
    mocks.listReleaseNotesVersions.mockResolvedValue(["0.9.0", "0.8.0"]);
    mocks.fetchReleaseMetadata.mockResolvedValue(
      makeMetadata([
        { version: "0.9.0", publishedAt: "2026-05-01T00:00:00Z", body: "## v0.9.0\n\nNew stuff" },
        { version: "0.8.0", publishedAt: "2026-04-15T00:00:00Z", body: "## v0.8.0\n\nOld stuff" },
      ]),
    );
    mocks.getReleaseNotesForVersion.mockImplementation(async (version: string) => ({
      version,
      body: `# Release ${version}\n\nBundled notes for ${version}`,
      source: "bundled_resource",
    }));
    mocks.getVersion.mockResolvedValue("0.8.0");
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders nothing when closed", () => {
    render(<ReleaseNotesDialog open={false} onClose={vi.fn()} />);
    expect(screen.queryByTestId("release-notes-dialog-body")).not.toBeInTheDocument();
  });

  it("loads version list and shows first version when opened", async () => {
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    expect(mocks.listReleaseNotesVersions).toHaveBeenCalled();
    expect(mocks.fetchReleaseMetadata).toHaveBeenCalled();
    expect(mocks.getVersion).toHaveBeenCalled();
    expect(screen.getByTestId("release-notes-dialog-body")).toBeInTheDocument();
  });

  it("shows sidebar with version buttons", async () => {
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    expect(screen.getAllByText("v0.9.0").length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText("v0.8.0").length).toBeGreaterThanOrEqual(1);
  });

  it("marks current app version in sidebar", async () => {
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    expect(screen.getByText("current")).toBeInTheDocument();
  });

  it("loads bundled notes for selected version", async () => {
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    expect(mocks.getReleaseNotesForVersion).toHaveBeenCalledWith("0.9.0");
  });

  it("clicking a sidebar version loads its notes", async () => {
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    fireEvent.click(screen.getByText("v0.8.0"));
    await flushAll();

    expect(mocks.getReleaseNotesForVersion).toHaveBeenCalledWith("0.8.0");
  });

  it("checks the persisted channel without inferring an update from history", async () => {
    const onCheckForUpdates = vi.fn();
    render(
      <ReleaseNotesDialog
        open={true}
        onClose={vi.fn()}
        onCheckForUpdates={onCheckForUpdates}
      />,
    );
    await flushAll();

    const updateButton = screen.getByTestId("release-notes-check-updates-button");
    expect(updateButton).toHaveTextContent("Check Stable for updates");

    fireEvent.click(updateButton);
    expect(onCheckForUpdates).toHaveBeenCalledWith("stable");
  });

  it("does not infer a version-specific CTA when already on the newest history row", async () => {
    mocks.getVersion.mockResolvedValue("0.9.0");
    render(
      <ReleaseNotesDialog
        open={true}
        onClose={vi.fn()}
        onCheckForUpdates={vi.fn()}
      />,
    );
    await flushAll();

    expect(screen.getByTestId("release-notes-check-updates-button")).toHaveTextContent(
      "Check Stable for updates",
    );
    expect(screen.queryByText(/Update to v/)).not.toBeInTheDocument();
  });

  it("does not show a check action without onCheckForUpdates prop", async () => {
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    expect(screen.queryByTestId("release-notes-check-updates-button")).not.toBeInTheDocument();
  });

  it("uses initialVersion to select starting version", async () => {
    render(
      <ReleaseNotesDialog open={true} onClose={vi.fn()} initialVersion="0.8.0" />,
    );
    await flushAll();

    expect(mocks.getReleaseNotesForVersion).toHaveBeenCalledWith("0.8.0");
  });

  it("falls back to first version when initialVersion not found", async () => {
    render(
      <ReleaseNotesDialog open={true} onClose={vi.fn()} initialVersion="99.99.99" />,
    );
    await flushAll();

    expect(mocks.getReleaseNotesForVersion).toHaveBeenCalledWith("0.9.0");
  });

  it("seeds update notes when initialVersion is missing from release index", async () => {
    const onCheckForUpdates = vi.fn();
    render(
      <ReleaseNotesDialog
        open={true}
        onClose={vi.fn()}
        initialVersion="0.9.1"
        initialBody="## RalphX.app 0.9.1\n\nUpdate-only notes"
        initialContext="update"
        initialChannel="nightly"
        onCheckForUpdates={onCheckForUpdates}
      />,
    );
    await flushAll();

    expect(screen.getByRole("heading", { name: /v0\.9\.1/ })).toBeInTheDocument();
    expect(screen.getByText(/Update-only notes/)).toBeInTheDocument();
    expect(screen.getByTestId("release-notes-use-channel-button")).toHaveTextContent(
      "Use Nightly",
    );
    expect(mocks.getReleaseNotesForVersion).not.toHaveBeenCalledWith("0.9.1");
  });

  it("shows context label for update context", async () => {
    render(
      <ReleaseNotesDialog
        open={true}
        onClose={vi.fn()}
        initialContext="update"
      />,
    );
    await flushAll();

    expect(screen.getByText("Available update")).toBeInTheDocument();
  });

  it("shows context label for default context", async () => {
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    expect(screen.getByText("Release History")).toBeInTheDocument();
  });

  it("browses Stable and Nightly histories without changing the update preference", async () => {
    mocks.fetchReleaseMetadata.mockResolvedValue(
      makeMetadata([
        { version: "1.1.0", publishedAt: "2026-06-01T00:00:00Z", body: "Nightly notes", prerelease: true },
        { version: "1.0.0", publishedAt: "2026-05-01T00:00:00Z", body: "Stable notes" },
      ]),
    );
    mocks.listReleaseNotesVersions.mockResolvedValue(["1.1.0", "1.0.0", "0.9.0"]);

    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    expect(screen.getByRole("tab", { name: /Stable/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.queryByText("v1.1.0")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("tab", { name: /Nightly/ }));
    await flushAll();

    expect(screen.getByRole("tab", { name: /Nightly/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getAllByText("v1.1.0").length).toBeGreaterThanOrEqual(1);
    expect(screen.queryByText("v1.0.0")).not.toBeInTheDocument();
    expect(mocks.setUpdateChannel).not.toHaveBeenCalled();
  });

  it("persists an explicit channel switch and closes only after success", async () => {
    const onClose = vi.fn();
    mocks.setUpdateChannel.mockImplementation(
      (_channel: UpdateChannel, options: { onSuccess?: () => void }) => {
        options.onSuccess?.();
      },
    );
    mocks.fetchReleaseMetadata.mockResolvedValue(
      makeMetadata([
        { version: "1.1.0", publishedAt: "2026-06-01T00:00:00Z", prerelease: true },
        { version: "1.0.0", publishedAt: "2026-05-01T00:00:00Z" },
      ]),
    );

    render(<ReleaseNotesDialog open={true} onClose={onClose} />);
    await flushAll();
    fireEvent.click(screen.getByRole("tab", { name: /Nightly/ }));
    fireEvent.click(screen.getByTestId("release-notes-use-channel-button"));

    expect(mocks.setUpdateChannel).toHaveBeenCalledWith(
      "nightly",
      expect.objectContaining({ onSuccess: expect.any(Function) }),
    );
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("shows pending channel-switch state without claiming success", async () => {
    mocks.isUpdateChannelSaving = true;
    mocks.fetchReleaseMetadata.mockResolvedValue(
      makeMetadata([
        { version: "1.1.0", publishedAt: "2026-06-01T00:00:00Z", prerelease: true },
      ]),
    );
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();
    fireEvent.click(screen.getByRole("tab", { name: /Nightly/ }));

    expect(screen.getByTestId("release-notes-use-channel-button")).toBeDisabled();
    expect(screen.getByTestId("release-notes-use-channel-button")).toHaveTextContent(
      "Switching...",
    );
  });

  it("keeps the dialog open and surfaces a channel-switch failure", async () => {
    const onClose = vi.fn();
    mocks.updateChannelSaveError = new Error("write failed");
    mocks.fetchReleaseMetadata.mockResolvedValue(
      makeMetadata([
        { version: "1.1.0", publishedAt: "2026-06-01T00:00:00Z", prerelease: true },
      ]),
    );
    render(<ReleaseNotesDialog open={true} onClose={onClose} />);
    await flushAll();
    fireEvent.click(screen.getByRole("tab", { name: /Nightly/ }));

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Could not switch update channels",
    );
    expect(onClose).not.toHaveBeenCalled();
  });

  it("opens seeded update notes on the notification channel", async () => {
    mocks.fetchReleaseMetadata.mockResolvedValue(
      makeMetadata([
        { version: "1.1.0", publishedAt: "2026-06-01T00:00:00Z", prerelease: true },
        { version: "1.0.0", publishedAt: "2026-05-01T00:00:00Z" },
      ]),
    );

    render(
      <ReleaseNotesDialog
        open={true}
        onClose={vi.fn()}
        initialVersion="1.1.1"
        initialBody="Seeded nightly notification"
        initialContext="update"
        initialChannel="nightly"
      />,
    );
    await flushAll();

    expect(screen.getByRole("tab", { name: /Nightly/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByText("Seeded nightly notification")).toBeInTheDocument();
  });

  it("uses authoritative metadata when a captured Nightly seed was promoted to Stable", async () => {
    mocks.fetchReleaseMetadata.mockResolvedValue(
      makeMetadata([
        {
          version: "1.1.0",
          publishedAt: "2026-06-01T00:00:00Z",
          body: "Promoted Stable notes",
          prerelease: false,
        },
      ]),
    );

    render(
      <ReleaseNotesDialog
        open={true}
        onClose={vi.fn()}
        initialVersion="1.1.0"
        initialBody="Seeded promoted notes"
        initialContext="update"
        initialChannel="nightly"
      />,
    );
    await flushAll();

    expect(screen.getByRole("tab", { name: /Stable/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByText("Seeded promoted notes")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: /Nightly/ }));
    expect(screen.queryByRole("heading", { name: "v1.1.0" })).not.toBeInTheDocument();
  });

  it("uses authoritative metadata to open a Nightly seed despite a Stable preference", async () => {
    mocks.fetchReleaseMetadata.mockResolvedValue(
      makeMetadata([
        {
          version: "1.1.0",
          publishedAt: "2026-06-01T00:00:00Z",
          prerelease: true,
        },
      ]),
    );

    render(
      <ReleaseNotesDialog
        open={true}
        onClose={vi.fn()}
        initialVersion="1.1.0"
        initialBody="Nightly seed"
        initialChannel="stable"
      />,
    );
    await flushAll();

    expect(screen.getByRole("tab", { name: /Nightly/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("does not override an explicit tab choice when metadata resolves later", async () => {
    let resolveMetadata!: (metadata: Map<string, ReleaseMetadata>) => void;
    mocks.fetchReleaseMetadata.mockReturnValue(
      new Promise<Map<string, ReleaseMetadata>>((resolve) => {
        resolveMetadata = resolve;
      }),
    );

    render(
      <ReleaseNotesDialog
        open={true}
        onClose={vi.fn()}
        initialVersion="1.1.0"
        initialBody="Late seed"
        initialChannel="stable"
      />,
    );
    await flushAll();
    fireEvent.click(screen.getByRole("tab", { name: /Nightly/ }));

    await act(async () => {
      resolveMetadata(
        makeMetadata([
          {
            version: "1.1.0",
            publishedAt: "2026-06-01T00:00:00Z",
            prerelease: false,
          },
        ]),
      );
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(screen.getByRole("tab", { name: /Nightly/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("fails closed when release classification is unavailable", async () => {
    mocks.fetchReleaseMetadata.mockResolvedValue(
      new ReleaseMetadataSnapshot([], "unavailable"),
    );

    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Release history is temporarily unavailable",
    );
    expect(screen.queryByText("v0.9.0")).not.toBeInTheDocument();
  });

  it("keeps seeded notes visible when release classification is unavailable", async () => {
    mocks.fetchReleaseMetadata.mockResolvedValue(
      new ReleaseMetadataSnapshot([], "unavailable"),
    );

    render(
      <ReleaseNotesDialog
        open={true}
        onClose={vi.fn()}
        initialVersion="1.1.0"
        initialBody="Authoritative notification notes"
        initialChannel="nightly"
      />,
    );
    await flushAll();

    expect(screen.getByText("Authoritative notification notes")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Release history is temporarily unavailable",
    );
  });

  it("uses stale last-known-good classification and surfaces its state", async () => {
    const stale = new ReleaseMetadataSnapshot(
      makeMetadata([
        {
          version: "1.1.0",
          publishedAt: "2026-06-01T00:00:00Z",
          prerelease: true,
        },
      ]),
      "stale",
    );
    mocks.fetchReleaseMetadata.mockResolvedValue(stale);

    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();
    fireEvent.click(screen.getByRole("tab", { name: /Nightly/ }));

    expect(screen.getByRole("heading", { name: "v1.1.0" })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Showing last-known release classification",
    );
  });

  it("supports roving keyboard navigation across channel tabs", async () => {
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    const stable = screen.getByRole("tab", { name: /Stable/ });
    const nightly = screen.getByRole("tab", { name: /Nightly/ });
    expect(stable).toHaveAttribute("tabindex", "0");
    expect(nightly).toHaveAttribute("tabindex", "-1");
    expect(stable).toHaveAttribute("aria-controls", "release-notes-panel");

    stable.focus();
    fireEvent.keyDown(stable, { key: "ArrowRight" });

    expect(nightly).toHaveFocus();
    expect(nightly).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel")).toHaveAttribute(
      "aria-labelledby",
      "release-notes-tab-nightly",
    );
  });

  it("falls back to GitHub body when bundled notes fail to load", async () => {
    mocks.getReleaseNotesForVersion.mockRejectedValue(new Error("not found"));
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    expect(screen.getByTestId("release-notes-dialog-body")).toBeInTheDocument();
  });

  it("uses GitHub body for non-bundled versions", async () => {
    mocks.listReleaseNotesVersions.mockResolvedValue(["0.8.0"]);
    mocks.fetchReleaseMetadata.mockResolvedValue(
      makeMetadata([
        { version: "0.9.0", publishedAt: "2026-05-01T00:00:00Z", body: "GitHub-only notes" },
        { version: "0.8.0", publishedAt: "2026-04-15T00:00:00Z", body: "Bundled" },
      ]),
    );
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    // 0.9.0 is in metadata but not bundled, so click it via button
    const buttons = screen.getAllByText("v0.9.0");
    const sidebarButton = buttons.find((el) => el.closest("button"));
    fireEvent.click(sidebarButton!);
    await flushAll();

    // Should not call getReleaseNotesForVersion for non-bundled
    const calls = mocks.getReleaseNotesForVersion.mock.calls;
    const v090Calls = calls.filter((c: string[]) => c[0] === "0.9.0");
    expect(v090Calls).toHaveLength(0);
  });

  it("resets state when dialog closes", async () => {
    const { rerender } = render(
      <ReleaseNotesDialog open={true} onClose={vi.fn()} />,
    );
    await flushAll();

    rerender(<ReleaseNotesDialog open={false} onClose={vi.fn()} />);
    await flushAll();

    expect(screen.queryByTestId("release-notes-dialog-body")).not.toBeInTheDocument();
  });

  it("shows empty state when no versions available", async () => {
    mocks.listReleaseNotesVersions.mockResolvedValue([]);
    mocks.fetchReleaseMetadata.mockResolvedValue(new Map());
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    expect(screen.queryByText(/v\d/)).not.toBeInTheDocument();
  });

  it("keeps GitHub history available when the bundled version index fails", async () => {
    mocks.listReleaseNotesVersions.mockRejectedValue(new Error("index error"));
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    expect(
      screen.getByRole("heading", { name: "v0.9.0" }),
    ).toBeInTheDocument();
    expect(screen.getByTestId("markdown")).toHaveTextContent("New stuff");
    expect(mocks.getReleaseNotesForVersion).not.toHaveBeenCalled();
  });

  it("does not re-fetch notes for already loaded version on click", async () => {
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    // First version auto-loaded
    expect(mocks.getReleaseNotesForVersion).toHaveBeenCalledTimes(1);

    // Click the same version again via sidebar button
    const buttons = screen.getAllByText("v0.9.0");
    const sidebarButton = buttons.find((el) => el.closest("button"));
    fireEvent.click(sidebarButton!);
    await flushAll();

    // Should not fetch again
    expect(mocks.getReleaseNotesForVersion).toHaveBeenCalledTimes(1);
  });

  it("displays month group headers in sidebar", async () => {
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    expect(screen.getByText("May 2026")).toBeInTheDocument();
    expect(screen.getByText("April 2026")).toBeInTheDocument();
  });

  it("shows date for non-current versions in sidebar", async () => {
    mocks.getVersion.mockResolvedValue("0.8.0");
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    // 0.9.0 is not the current version, should show date
    expect(screen.getAllByText("May 1").length).toBeGreaterThanOrEqual(1);
  });

  it("strips github-release-metadata markers from body", async () => {
    mocks.getReleaseNotesForVersion.mockResolvedValue({
      version: "0.9.0",
      body: "Real content\n<!-- github-release-metadata:start -->\nHidden\n<!-- github-release-metadata:end -->\n",
      source: "bundled_resource",
    });
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    const body = screen.getByTestId("release-notes-dialog-body");
    expect(body.textContent).toContain("Real content");
    expect(body.textContent).not.toContain("github-release-metadata");
  });

  it("shows 'not available' message when body is null", async () => {
    mocks.getReleaseNotesForVersion.mockResolvedValue({
      version: "0.9.0",
      body: null,
      source: "bundled_resource",
    });
    mocks.fetchReleaseMetadata.mockResolvedValue(
      makeMetadata([
        { version: "0.9.0", publishedAt: "2026-05-01T00:00:00Z", body: null },
        { version: "0.8.0", publishedAt: "2026-04-15T00:00:00Z", body: null },
      ]),
    );
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    expect(
      screen.getByText("Release notes not available for this version."),
    ).toBeInTheDocument();
  });

  it("shows version heading with date in content area", async () => {
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    expect(screen.getByRole("heading", { name: /v0\.9\.0/ })).toBeInTheDocument();
  });

  it("shows loading spinner while version list is loading", async () => {
    let resolveVersions!: (value: string[]) => void;
    mocks.listReleaseNotesVersions.mockReturnValue(
      new Promise<string[]>((resolve) => {
        resolveVersions = resolve;
      }),
    );

    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);

    const body = screen.getByTestId("release-notes-dialog-body");
    expect(body).toBeInTheDocument();

    await act(async () => {
      resolveVersions(["0.9.0"]);
      await Promise.resolve();
    });
  });

  it("shows loading state while fetching version content", async () => {
    let resolveNotes!: (value: { version: string; body: string; source: string }) => void;
    mocks.getReleaseNotesForVersion.mockReturnValue(
      new Promise((resolve) => {
        resolveNotes = resolve;
      }),
    );

    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    await act(async () => {
      resolveNotes({ version: "0.9.0", body: "Done loading", source: "bundled_resource" });
      await Promise.resolve();
      await Promise.resolve();
    });

    const body = screen.getByTestId("release-notes-dialog-body");
    expect(body.textContent).toContain("Done loading");
  });

  it("falls back to GitHub body when clicking a version whose bundled fetch fails", async () => {
    mocks.getReleaseNotesForVersion
      .mockResolvedValueOnce({
        version: "0.9.0",
        body: "# Release 0.9.0\n\nBundled",
        source: "bundled_resource",
      })
      .mockRejectedValueOnce(new Error("disk error"));

    mocks.fetchReleaseMetadata.mockResolvedValue(
      makeMetadata([
        { version: "0.9.0", publishedAt: "2026-05-01T00:00:00Z", body: "GH 0.9.0" },
        { version: "0.8.0", publishedAt: "2026-04-15T00:00:00Z", body: "GH fallback for 0.8.0" },
      ]),
    );

    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    expect(screen.getByTestId("release-notes-dialog-body")).toBeInTheDocument();

    fireEvent.click(screen.getByText("v0.8.0"));
    await flushAll();

    const body = screen.getByTestId("release-notes-dialog-body");
    expect(body.textContent).toContain("GH fallback for 0.8.0");
  });
});
