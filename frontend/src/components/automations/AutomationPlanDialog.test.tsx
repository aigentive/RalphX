import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AutomationPlanDialog } from "./AutomationPlanDialog";
import type { ArtifactResponse } from "@/api/artifacts";

const { useArtifactMock, reactMarkdownRenderSpy } = vi.hoisted(() => ({
  useArtifactMock: vi.fn(),
  reactMarkdownRenderSpy: vi.fn(),
}));

vi.mock("@/hooks/useArtifacts", () => ({
  useArtifact: (...args: unknown[]) => useArtifactMock(...args),
}));

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
  id: "artifact-plan-1",
  name: "Run 8 plan",
  artifact_type: "specification",
  content_type: "inline",
  content: "## Plan\nDo the migration in two steps.",
  created_at: "2026-07-05T00:00:00Z",
  created_by: "orchestrator",
  version: 1,
  bucket_id: null,
  task_id: null,
  process_id: null,
  derived_from: [],
  ...overrides,
});

describe("AutomationPlanDialog", () => {
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

  it("does not fetch while closed", () => {
    render(
      <AutomationPlanDialog
        planArtifactId="artifact-plan-1"
        open={false}
        onOpenChange={() => undefined}
      />,
    );

    expect(useArtifactMock).not.toHaveBeenCalled();
    expect(screen.queryByTestId("automation-plan-dialog")).not.toBeInTheDocument();
  });

  it("paints the shell immediately and hydrates markdown after paint", async () => {
    useArtifactMock.mockReturnValue({
      data: artifact(),
      isLoading: false,
      isError: false,
    });

    render(
      <AutomationPlanDialog
        planArtifactId="artifact-plan-1"
        title="B1 — Skill schema"
        open
        onOpenChange={() => undefined}
      />,
    );

    // Shell chrome is synchronous.
    expect(screen.getByTestId("automation-plan-dialog")).toBeInTheDocument();
    expect(screen.getByText("Run plan")).toBeInTheDocument();
    expect(screen.getByText("B1 — Skill schema")).toBeInTheDocument();
    // Markdown chunk has not evaluated in the same commit.
    expect(reactMarkdownRenderSpy).not.toHaveBeenCalled();
    // Plain-text fallback keeps content readable before hydration.
    expect(screen.getByTestId("automation-plan-dialog-markdown")).toHaveTextContent(
      "Do the migration in two steps.",
    );

    await waitFor(() => {
      expect(screen.getByTestId("rendered-markdown")).toBeInTheDocument();
    });
    expect(useArtifactMock).toHaveBeenCalledWith("artifact-plan-1");
  });

  it("supports a context-specific heading without changing the default", () => {
    render(
      <AutomationPlanDialog
        planArtifactId={null}
        heading="Automation spec"
        title="Linked specification"
        open
        onOpenChange={() => undefined}
      />,
    );

    expect(screen.getByText("Automation spec")).toBeInTheDocument();
    expect(screen.queryByText("Run plan")).not.toBeInTheDocument();
    expect(screen.getByText("Linked specification")).toBeInTheDocument();
  });

  it("shows a loading skeleton while the artifact fetch is in flight", () => {
    useArtifactMock.mockReturnValue({
      data: null,
      isLoading: true,
      isError: false,
    });

    render(
      <AutomationPlanDialog
        planArtifactId="artifact-plan-1"
        open
        onOpenChange={() => undefined}
      />,
    );

    expect(screen.getByTestId("automation-plan-dialog-loading")).toBeInTheDocument();
  });

  it("fails closed with an explicit error when the artifact cannot be read", () => {
    useArtifactMock.mockReturnValue({
      data: null,
      isLoading: false,
      isError: true,
    });

    render(
      <AutomationPlanDialog
        planArtifactId="artifact-plan-1"
        open
        onOpenChange={() => undefined}
      />,
    );

    expect(screen.getByTestId("automation-plan-dialog-error")).toBeInTheDocument();
    expect(screen.queryByTestId("automation-plan-dialog-markdown")).not.toBeInTheDocument();
  });

  it("fails closed when a null artifact resolves without error", () => {
    useArtifactMock.mockReturnValue({
      data: null,
      isLoading: false,
      isError: false,
    });

    render(
      <AutomationPlanDialog
        planArtifactId="artifact-plan-1"
        open
        onOpenChange={() => undefined}
      />,
    );

    expect(screen.getByTestId("automation-plan-dialog-error")).toBeInTheDocument();
  });

  it("shows an explicit error when no plan artifact id is linked", () => {
    render(
      <AutomationPlanDialog
        planArtifactId={null}
        open
        onOpenChange={() => undefined}
      />,
    );

    expect(useArtifactMock).not.toHaveBeenCalled();
    expect(screen.getByTestId("automation-plan-dialog-error")).toBeInTheDocument();
  });
});
