import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, renderHook } from "@testing-library/react";
import { EventProvider, useEventBus } from "./EventProvider";
import type { ReactNode } from "react";

// Mock the event hooks
vi.mock("@/hooks/useEvents", () => ({
  useTaskEvents: vi.fn(),
  useSupervisorAlerts: vi.fn(),
  useReviewEvents: vi.fn(),
  useFileChangeEvents: vi.fn(),
  useAgentEvents: vi.fn(),
  useProposalEvents: vi.fn(),
  useStepEvents: vi.fn(),
  useExecutionErrorEvents: vi.fn(),
  useRecoveryPromptEvents: vi.fn(),
}));

vi.mock("@/hooks/useIdeationEvents", () => ({
  useIdeationEvents: vi.fn(),
}));

vi.mock("@/hooks/useEvents.planArtifact", () => ({
  usePlanArtifactEvents: vi.fn(),
}));

vi.mock("@/hooks/useVerificationEvents", () => ({
  useVerificationEvents: vi.fn(),
}));

vi.mock("@/hooks/useFreshnessBlockedNotification", () => ({
  useFreshnessBlockedNotification: vi.fn(),
}));

vi.mock("@/hooks/useGitAuthStartupNotification", () => ({
  useGitAuthStartupNotification: vi.fn(),
}));

vi.mock("@/hooks/useGlobalAgentLifecycle", () => ({
  useGlobalAgentLifecycle: vi.fn(),
}));

// Mock the event bus module
vi.mock("@/lib/event-bus", () => ({
  createEventBus: vi.fn(() => ({
    subscribe: vi.fn(() => vi.fn()),
    emit: vi.fn(),
  })),
}));

import {
  useTaskEvents,
  useSupervisorAlerts,
  useReviewEvents,
  useFileChangeEvents,
} from "@/hooks/useEvents";

describe("EventProvider", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should render children", () => {
    render(
      <EventProvider>
        <div data-testid="child">Hello World</div>
      </EventProvider>
    );

    expect(screen.getByTestId("child")).toHaveTextContent("Hello World");
  });

  it("should call useTaskEvents hook", () => {
    render(
      <EventProvider>
        <div>Test</div>
      </EventProvider>
    );

    expect(useTaskEvents).toHaveBeenCalled();
  });

  it("should call useSupervisorAlerts hook", () => {
    render(
      <EventProvider>
        <div>Test</div>
      </EventProvider>
    );

    expect(useSupervisorAlerts).toHaveBeenCalled();
  });

  it("should call useReviewEvents hook", () => {
    render(
      <EventProvider>
        <div>Test</div>
      </EventProvider>
    );

    expect(useReviewEvents).toHaveBeenCalled();
  });

  it("should call useFileChangeEvents hook", () => {
    render(
      <EventProvider>
        <div>Test</div>
      </EventProvider>
    );

    expect(useFileChangeEvents).toHaveBeenCalled();
  });

  it("should render multiple children", () => {
    render(
      <EventProvider>
        <div data-testid="child1">First</div>
        <div data-testid="child2">Second</div>
      </EventProvider>
    );

    expect(screen.getByTestId("child1")).toBeInTheDocument();
    expect(screen.getByTestId("child2")).toBeInTheDocument();
  });

  it("should render nested components", () => {
    render(
      <EventProvider>
        <div data-testid="outer">
          <span data-testid="inner">Nested content</span>
        </div>
      </EventProvider>
    );

    expect(screen.getByTestId("outer")).toContainElement(
      screen.getByTestId("inner")
    );
  });
});

describe("useEventBus", () => {
  const wrapper = ({ children }: { children: ReactNode }) => (
    <EventProvider>{children}</EventProvider>
  );

  it("should return event bus when used within EventProvider", () => {
    const { result } = renderHook(() => useEventBus(), { wrapper });

    expect(result.current).toBeDefined();
    expect(result.current.subscribe).toBeDefined();
    expect(result.current.emit).toBeDefined();
  });

  it("should throw error when used outside EventProvider", () => {
    // Suppress console.error for this test since we expect an error
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});

    expect(() => {
      renderHook(() => useEventBus());
    }).toThrow("useEventBus must be used within an EventProvider");

    consoleSpy.mockRestore();
  });

  it("should return the same event bus instance on re-renders", () => {
    const { result, rerender } = renderHook(() => useEventBus(), { wrapper });

    const firstBus = result.current;
    rerender();
    const secondBus = result.current;

    expect(firstBus).toBe(secondBus);
  });
});
