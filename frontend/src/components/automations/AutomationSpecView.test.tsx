import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AutomationSpecView } from "./AutomationSpecView";
import type { ArtifactResponse } from "@/api/artifacts";

const { useArtifactMock, reactMarkdownRenderSpy } = vi.hoisted(() => ({
  useArtifactMock: vi.fn(),
  reactMarkdownRenderSpy: vi.fn(),
}));

vi.mock("@/hooks/useArtifacts", () => ({
  useArtifact: (...args: unknown[]) => useArtifactMock(...args),
}));

// Keep the heavy markdown chunk out of the collapsed-shell path: prove it only
// evaluates after the user expands the spec.
vi.mock("react-markdown", () => ({
  default: ({ children }: { children: string }) => {
    reactMarkdownRenderSpy(children);
    return <div data-testid="rendered-markdown">{children}</div>;
  },
}));
vi.mock("remark-gfm", () => ({ default: () => undefined }));
vi.mock("@/components/Chat/MessageItem.markdown", () => ({
  markdownComponents: {},
}));

const artifact = (overrides: Partial<ArtifactResponse> = {}): ArtifactResponse => ({
  id: "artifact-spec-1",
  name: "Migration loop spec",
  artifact_type: "specification",
  content_type: "inline",
  content: "## Phase 1\nBuild the shared context model.\n\n## Phase 2\nWire it up.",
  created_at: "2026-07-05T00:00:00Z",
  created_by: "setup-agent",
  version: 1,
  bucket_id: null,
  task_id: null,
  process_id: null,
  derived_from: [],
  ...overrides,
});

describe("AutomationSpecView", () => {
  beforeEach(() => {
    vi.stubGlobal(
      "requestAnimationFrame",
      (cb: FrameRequestCallback): number =>
        window.setTimeout(() => cb(performance.now()), 0),
    );
    vi.stubGlobal("cancelAnimationFrame", (handle: number): void => {
      window.clearTimeout(handle);
    });
    useArtifactMock.mockReset().mockReturnValue({
      data: null,
      isLoading: false,
      isError: false,
    });
    reactMarkdownRenderSpy.mockReset();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("renders a synchronous collapsed shell without hydrating markdown", () => {
    useArtifactMock.mockReturnValue({
      data: artifact(),
      isLoading: false,
      isError: false,
    });

    render(<AutomationSpecView specArtifactId="artifact-spec-1" />);

    // Shell chrome paints immediately.
    expect(
      screen.getByText("The specification this automation implements."),
    ).toBeInTheDocument();
    expect(screen.getByText("Migration loop spec")).toBeInTheDocument();
    // Excerpt (heading markers stripped) is visible without expanding.
    expect(screen.getByText(/Build the shared context model\./)).toBeInTheDocument();
    // Word count meta.
    expect(screen.getByText(/\bwords\b/)).toBeInTheDocument();
    // Expand affordance.
    expect(screen.getByTestId("automation-spec-toggle")).toHaveTextContent(
      "Show full spec",
    );
    expect(screen.getByTestId("automation-spec-toggle")).toHaveClass(
      "border-0",
      "bg-transparent",
      "p-0",
      "text-xs",
    );

    // Heavy markdown must NOT be mounted or evaluated before expansion.
    expect(screen.queryByTestId("automation-spec-markdown")).not.toBeInTheDocument();
    expect(screen.queryByTestId("rendered-markdown")).not.toBeInTheDocument();
    expect(reactMarkdownRenderSpy).not.toHaveBeenCalled();
  });

  it("strips markdown tokens from the clamped three-line excerpt", () => {
    useArtifactMock.mockReturnValue({
      data: artifact({
        content: "## Phase 1\nBuild the **shared** [context model](https://example.com).\n- Keep it focused.\n> Ship it.\nA fourth line.",
      }),
      isLoading: false,
      isError: false,
    });

    render(<AutomationSpecView specArtifactId="artifact-spec-1" />);

    const excerpt = screen.getByTestId("automation-spec-excerpt");
    expect(excerpt).toHaveClass("line-clamp-3", "text-sm", "leading-6");
    expect(excerpt).toHaveTextContent("Build the shared context model.");
    expect(excerpt).not.toHaveTextContent("**");
    expect(excerpt).not.toHaveTextContent("](");
    expect(excerpt).not.toHaveTextContent("A fourth line.");
  });

  it("hydrates the markdown renderer only after expanding past a paint boundary", async () => {
    useArtifactMock.mockReturnValue({
      data: artifact(),
      isLoading: false,
      isError: false,
    });

    render(<AutomationSpecView specArtifactId="artifact-spec-1" />);

    await userEvent.click(screen.getByTestId("automation-spec-toggle"));

    // Region shell appears synchronously with a plain-text fallback.
    const region = screen.getByTestId("automation-spec-markdown");
    expect(region).toHaveClass("max-w-3xl", "text-sm", "leading-6");
    expect(region).toHaveTextContent("Build the shared context model.");
    expect(screen.getByTestId("automation-spec-toggle")).toHaveTextContent(
      "Hide spec",
    );

    // The lazy markdown renderer mounts after the paint boundary resolves.
    await waitFor(() =>
      expect(screen.getByTestId("rendered-markdown")).toBeInTheDocument(),
    );
    expect(reactMarkdownRenderSpy).toHaveBeenCalled();
  });

  it("shows a loading state while the spec artifact resolves", () => {
    useArtifactMock.mockReturnValue({
      data: null,
      isLoading: true,
      isError: false,
    });

    render(<AutomationSpecView specArtifactId="artifact-spec-1" />);

    expect(screen.getByText("Loading spec...")).toBeInTheDocument();
    expect(screen.queryByTestId("automation-spec-toggle")).not.toBeInTheDocument();
  });

  it("shows the empty fallback when no spec is linked", () => {
    render(<AutomationSpecView specArtifactId={null} />);

    expect(screen.getByText("No spec linked yet.")).toBeInTheDocument();
    expect(useArtifactMock).toHaveBeenCalledWith("");
  });

  it("shows the no-content fallback for an empty spec body", () => {
    useArtifactMock.mockReturnValue({
      data: artifact({ content: "   " }),
      isLoading: false,
      isError: false,
    });

    render(<AutomationSpecView specArtifactId="artifact-spec-1" />);

    expect(screen.getByText("Spec has no content yet.")).toBeInTheDocument();
    expect(screen.queryByTestId("automation-spec-toggle")).not.toBeInTheDocument();
  });
});
