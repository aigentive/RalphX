import React from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ReleaseNotesDialog } from "./ReleaseNotesDialog";
import type { ReleaseMetadata } from "@/api/release-notes";

const mocks = vi.hoisted(() => ({
  listReleaseNotesVersions: vi.fn(),
  fetchReleaseMetadata: vi.fn(),
  getReleaseNotesForVersion: vi.fn(),
  getVersion: vi.fn(),
  compareSemverDesc: vi.fn(),
  mergeVersionLists: vi.fn(),
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
  entries: Array<{ version: string; publishedAt: string; body?: string | null }>,
): Map<string, ReleaseMetadata> {
  const map = new Map<string, ReleaseMetadata>();
  for (const e of entries) {
    map.set(e.version, {
      version: e.version,
      publishedAt: e.publishedAt,
      name: `v${e.version}`,
      body: e.body ?? null,
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

  it("shows update button when newer version available", async () => {
    const onRequestUpdate = vi.fn();
    render(
      <ReleaseNotesDialog
        open={true}
        onClose={vi.fn()}
        onRequestUpdate={onRequestUpdate}
      />,
    );
    await flushAll();

    const updateButton = screen.getByTestId("release-notes-update-button");
    expect(updateButton).toBeInTheDocument();
    expect(updateButton).toHaveTextContent("Update to v0.9.0");

    fireEvent.click(updateButton);
    expect(onRequestUpdate).toHaveBeenCalled();
  });

  it("does not show update button when on latest version", async () => {
    mocks.getVersion.mockResolvedValue("0.9.0");
    render(
      <ReleaseNotesDialog
        open={true}
        onClose={vi.fn()}
        onRequestUpdate={vi.fn()}
      />,
    );
    await flushAll();

    expect(screen.queryByTestId("release-notes-update-button")).not.toBeInTheDocument();
  });

  it("does not show update button without onRequestUpdate prop", async () => {
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    expect(screen.queryByTestId("release-notes-update-button")).not.toBeInTheDocument();
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

  it("handles loading failure gracefully", async () => {
    mocks.listReleaseNotesVersions.mockRejectedValue(new Error("network error"));
    render(<ReleaseNotesDialog open={true} onClose={vi.fn()} />);
    await flushAll();

    // Should not crash
    expect(screen.queryByTestId("release-notes-dialog-body")).toBeInTheDocument();
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
