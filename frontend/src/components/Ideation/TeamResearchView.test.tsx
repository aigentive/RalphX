/**
 * Tests for TeamResearchView - collapsible artifact cards rendered in the
 * "Team Research" tab. Covers empty state, summary header, collapsed/expanded
 * card states, lazy fetch flow (loading / success / error / empty content),
 * type-icon config selection, and the singular/plural artifact counter.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { TeamArtifactSummary } from "@/api/team";
import type { Artifact } from "@/types/artifact";

// ---------------------------------------------------------------------------
// Mocks
// ---------------------------------------------------------------------------

const getArtifactMock = vi.fn();

vi.mock("@/api/artifact", () => ({
  artifactApi: {
    get: (id: string) => getArtifactMock(id),
  },
}));

vi.mock("@/lib/theme-colors", () => ({
  withAlpha: (color: string, alpha: number) => `${color}/${alpha}`,
}));

import { TeamResearchView } from "./TeamResearchView";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeSummary(
  overrides: Partial<TeamArtifactSummary> = {},
): TeamArtifactSummary {
  return {
    id: "art-1",
    name: "Research note",
    artifact_type: "research",
    version: 1,
    content_preview: "Preview text",
    created_at: "2026-01-24T12:00:00Z",
    author_teammate: "Alice",
    ...overrides,
  };
}

function makeFullArtifact(
  overrides: Partial<Artifact> = {},
  contentText = "# Full markdown content",
): Artifact {
  return {
    id: "art-1",
    type: "research",
    name: "Research note",
    content: { type: "inline", text: contentText },
    metadata: {
      createdAt: "2026-01-24T12:00:00Z",
      createdBy: "Alice",
      version: 1,
    },
    derivedFrom: [],
    ...overrides,
  } as Artifact;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("TeamResearchView", () => {
  beforeEach(() => {
    getArtifactMock.mockReset();
  });

  it("renders the empty state when there are no artifacts", () => {
    render(<TeamResearchView artifacts={[]} sessionId="s1" />);
    expect(
      screen.getByText("No team research artifacts yet"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Artifacts will appear here once team research completes"),
    ).toBeInTheDocument();
  });

  it("renders the section header with singular count for one artifact", () => {
    render(
      <TeamResearchView
        artifacts={[makeSummary()]}
        sessionId="s1"
      />,
    );
    expect(screen.getByText("Team Research")).toBeInTheDocument();
    expect(screen.getByText("1 artifact")).toBeInTheDocument();
  });

  it("renders plural count for multiple artifacts and lists each card", () => {
    const artifacts = [
      makeSummary({ id: "a", name: "Alpha" }),
      makeSummary({ id: "b", name: "Beta", artifact_type: "analysis" }),
      makeSummary({ id: "c", name: "Gamma", artifact_type: "summary" }),
    ];
    render(<TeamResearchView artifacts={artifacts} sessionId="s1" />);
    expect(screen.getByText("3 artifacts")).toBeInTheDocument();
    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.getByText("Beta")).toBeInTheDocument();
    expect(screen.getByText("Gamma")).toBeInTheDocument();
  });

  it("renders unknown artifact_type using the default config without crashing", () => {
    render(
      <TeamResearchView
        artifacts={[
          makeSummary({ id: "x", name: "Mystery", artifact_type: "weird-type" }),
        ]}
        sessionId="s1"
      />,
    );
    expect(screen.getByText("Mystery")).toBeInTheDocument();
  });

  it("shows author in collapsed state when author_teammate is provided", () => {
    render(
      <TeamResearchView
        artifacts={[makeSummary({ author_teammate: "Alice" })]}
        sessionId="s1"
      />,
    );
    expect(screen.getByText("by Alice")).toBeInTheDocument();
  });

  it("does not crash when collapsed and author_teammate is null", () => {
    render(
      <TeamResearchView
        artifacts={[makeSummary({ author_teammate: null })]}
        sessionId="s1"
      />,
    );
    expect(screen.queryByText(/^by /)).not.toBeInTheDocument();
  });

  it("renders the version badge", () => {
    render(
      <TeamResearchView
        artifacts={[makeSummary({ version: 7 })]}
        sessionId="s1"
      />,
    );
    expect(screen.getByText("v7")).toBeInTheDocument();
  });

  it("expanding a card triggers artifactApi.get and renders fetched markdown", async () => {
    const user = userEvent.setup();
    let resolve: (a: Artifact) => void = () => {};
    getArtifactMock.mockImplementation(
      () => new Promise<Artifact>((r) => (resolve = r)),
    );

    render(
      <TeamResearchView
        artifacts={[makeSummary({ id: "art-1" })]}
        sessionId="s1"
      />,
    );

    await user.click(screen.getByRole("button", { name: /Research note/ }));

    // Loading indicator visible while pending
    expect(screen.getByText("Loading full content...")).toBeInTheDocument();
    expect(getArtifactMock).toHaveBeenCalledWith("art-1");

    const richMarkdown = [
      "# Heading 1",
      "## Heading 2",
      "### Heading 3",
      "",
      "Paragraph with [a link](https://example.com) and `inline code`.",
      "",
      "```ts",
      "const x = 1;",
      "```",
      "",
      "- bullet item",
      "",
      "1. numbered item",
      "",
      "> quoted text",
      "",
      "---",
      "",
      "| Col A | Col B |",
      "| --- | --- |",
      "| a | b |",
    ].join("\n");

    await act(async () => {
      resolve(makeFullArtifact({}, richMarkdown));
    });

    await waitFor(() => {
      expect(screen.getByText("Heading 1")).toBeInTheDocument();
    });
    // Custom markdown components rendered
    expect(screen.getByText("Heading 2")).toBeInTheDocument();
    expect(screen.getByText("Heading 3")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "a link" })).toHaveAttribute(
      "href",
      "https://example.com",
    );
    expect(screen.getByText("inline code")).toBeInTheDocument();
    expect(screen.getByText("bullet item")).toBeInTheDocument();
    expect(screen.getByText("numbered item")).toBeInTheDocument();
    expect(screen.getByText("quoted text")).toBeInTheDocument();
    // Table cells render
    expect(screen.getByText("Col A")).toBeInTheDocument();
    expect(screen.getByText("a")).toBeInTheDocument();
    // Expanded author label uses metadata.createdBy
    expect(screen.getByText("by Alice")).toBeInTheDocument();
  });

  it("renders error state when fetch rejects, including content_preview fallback", async () => {
    const user = userEvent.setup();
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    getArtifactMock.mockRejectedValue(new Error("boom"));

    render(
      <TeamResearchView
        artifacts={[
          makeSummary({
            id: "art-err",
            content_preview: "Fallback preview text",
          }),
        ]}
        sessionId="s1"
      />,
    );

    await user.click(screen.getByRole("button", { name: /Research note/ }));

    await waitFor(() => {
      expect(screen.getByText("Failed to load content")).toBeInTheDocument();
    });
    expect(screen.getByText("Fallback preview text")).toBeInTheDocument();
    expect(errorSpy).toHaveBeenCalled();
    errorSpy.mockRestore();
  });

  it("renders 'No content available' when fetched artifact has non-inline content", async () => {
    const user = userEvent.setup();
    const fileBacked: Artifact = {
      id: "art-1",
      type: "research",
      name: "Research note",
      content: { type: "file", path: "/tmp/x.md" },
      metadata: {
        createdAt: "2026-01-24T12:00:00Z",
        createdBy: "Alice",
        version: 1,
      },
      derivedFrom: [],
    } as Artifact;
    getArtifactMock.mockResolvedValue(fileBacked);

    render(
      <TeamResearchView
        artifacts={[makeSummary({ id: "art-1" })]}
        sessionId="s1"
      />,
    );

    await user.click(screen.getByRole("button", { name: /Research note/ }));

    await waitFor(() => {
      expect(screen.getByText("No content available")).toBeInTheDocument();
    });
  });

  it("does not refetch on subsequent expand/collapse cycles", async () => {
    const user = userEvent.setup();
    getArtifactMock.mockResolvedValue(makeFullArtifact({}, "Cached body"));

    render(
      <TeamResearchView
        artifacts={[makeSummary({ id: "art-1" })]}
        sessionId="s1"
      />,
    );

    const trigger = screen.getByRole("button", { name: /Research note/ });
    await user.click(trigger);
    await waitFor(() => {
      expect(screen.getByText("Cached body")).toBeInTheDocument();
    });

    // Collapse
    await user.click(trigger);
    // Expand again
    await user.click(trigger);

    expect(getArtifactMock).toHaveBeenCalledTimes(1);
  });

  it("each artifact card fetches its own content independently", async () => {
    const user = userEvent.setup();
    getArtifactMock.mockImplementation((id: string) =>
      Promise.resolve(makeFullArtifact({ id }, `body-${id}`)),
    );

    render(
      <TeamResearchView
        artifacts={[
          makeSummary({ id: "first", name: "First card" }),
          makeSummary({ id: "second", name: "Second card" }),
        ]}
        sessionId="s1"
      />,
    );

    await user.click(screen.getByRole("button", { name: /First card/ }));
    await waitFor(() => {
      expect(screen.getByText("body-first")).toBeInTheDocument();
    });

    await user.click(screen.getByRole("button", { name: /Second card/ }));
    await waitFor(() => {
      expect(screen.getByText("body-second")).toBeInTheDocument();
    });

    expect(getArtifactMock).toHaveBeenCalledTimes(2);
  });
});
