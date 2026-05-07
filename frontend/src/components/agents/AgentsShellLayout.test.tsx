import { createRef } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AgentsShellLayout } from "./AgentsShellLayout";

vi.mock("@/components/ui/ResizeHandle", () => ({
  ResizeHandle: ({
    isResizing,
    onDoubleClick,
    onMouseDown,
    testId,
  }: {
    isResizing: boolean;
    onDoubleClick: React.MouseEventHandler<HTMLDivElement>;
    onMouseDown: React.MouseEventHandler<HTMLDivElement>;
    testId?: string;
  }) => (
    <div
      role="separator"
      data-resizing={String(isResizing)}
      data-testid={testId}
      onDoubleClick={onDoubleClick}
      onMouseDown={onMouseDown}
    />
  ),
}));

vi.mock("./AgentsSidebar", () => ({
  AgentsSidebar: () => <div data-testid="mock-agents-sidebar" />,
}));

vi.mock("./AgentsSidebarVisibilityProvider", () => ({
  AgentsSidebarVisibilityProvider: ({
    children,
  }: {
    children: React.ReactNode;
  }) => <div data-testid="mock-sidebar-visibility-provider">{children}</div>,
}));

vi.mock("@/components/ui/tooltip", () => ({
  TooltipProvider: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="mock-tooltip-provider">{children}</div>
  ),
}));

type ShellProps = React.ComponentProps<typeof AgentsShellLayout>;

function buildProps(overrides: Partial<ShellProps> = {}): ShellProps {
  const splitContainerRef = createRef<HTMLDivElement>();
  const sidebarProps = {} as ShellProps["sidebarProps"];
  return {
    isSidebarCollapsed: false,
    isSidebarOverlayOpen: false,
    onCloseSidebarOverlay: vi.fn(),
    onToggleSidebarCollapse: vi.fn(),
    sidebarProps,
    sidebarWidth: 280,
    splitContainerRef,
    suppressSidebarTransition: { current: false },
    children: <div data-testid="shell-children">children</div>,
    ...overrides,
  };
}

describe("AgentsShellLayout", () => {
  it("paints the section with the app content background and renders children", () => {
    render(<AgentsShellLayout {...buildProps()} />);

    const section = screen.getByTestId("agents-view");
    expect(section.style.backgroundColor).toBe("var(--app-content-bg)");

    expect(screen.getByTestId("shell-children")).toBeInTheDocument();
    expect(screen.getByTestId("mock-agents-sidebar")).toBeInTheDocument();

    const splitContainer = screen.getByTestId("agents-split-container");
    expect(splitContainer.style.backgroundColor).toBe("var(--app-content-bg)");
  });

  it("uses an animated transition when suppressSidebarTransition is false", () => {
    render(<AgentsShellLayout {...buildProps()} />);

    const sidebarContainer = screen.getByTestId("agents-sidebar-container");
    expect(sidebarContainer.style.transition).toBe("width 300ms ease");
  });

  it("disables the sidebar transition when suppressSidebarTransition.current is true", () => {
    render(
      <AgentsShellLayout
        {...buildProps({ suppressSidebarTransition: { current: true } })}
      />
    );

    const sidebarContainer = screen.getByTestId("agents-sidebar-container");
    expect(sidebarContainer.style.transition).toBe("none");
  });

  it("restores the persistent desktop resize handle and applies dragged width", async () => {
    render(<AgentsShellLayout {...buildProps()} />);

    const sidebarContainer = screen.getByTestId("agents-sidebar-container");
    vi.spyOn(sidebarContainer, "getBoundingClientRect").mockReturnValue({
      bottom: 720,
      height: 720,
      left: 0,
      right: 280,
      top: 0,
      width: 280,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    } as DOMRect);

    const handle = screen.getByTestId("agents-sidebar-resize-handle");
    expect(handle).toHaveAttribute("data-resizing", "false");

    fireEvent.mouseDown(handle, { clientX: 280 });
    await waitFor(() => expect(handle).toHaveAttribute("data-resizing", "true"));
    expect(sidebarContainer.style.transition).toBe("none");

    fireEvent.mouseMove(document, { clientX: 360 });
    fireEvent.mouseUp(document);

    await waitFor(() => {
      expect(sidebarContainer.style.width).toBe("360px");
      expect(sidebarContainer.style.minWidth).toBe("360px");
      expect(handle).toHaveAttribute("data-resizing", "false");
    });
  });

  it("hides the desktop resize handle while the sidebar is collapsed or overlayed", () => {
    const { rerender } = render(
      <AgentsShellLayout
        {...buildProps({
          isSidebarCollapsed: true,
        })}
      />,
    );

    expect(screen.queryByTestId("agents-sidebar-resize-handle")).not.toBeInTheDocument();

    rerender(
      <AgentsShellLayout
        {...buildProps({
          isSidebarOverlayOpen: true,
        })}
      />,
    );

    expect(screen.queryByTestId("agents-sidebar-resize-handle")).not.toBeInTheDocument();
  });

  it("shows the toggle strip and triggers toggle on click + Enter/Space when collapsed", () => {
    const onToggle = vi.fn();
    render(
      <AgentsShellLayout
        {...buildProps({
          isSidebarCollapsed: true,
          onToggleSidebarCollapse: onToggle,
        })}
      />
    );

    const strip = screen.getByTestId("agents-sidebar-toggle-strip");
    expect(strip).toBeInTheDocument();

    fireEvent.click(strip);
    expect(onToggle).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(strip, { key: "Enter" });
    expect(onToggle).toHaveBeenCalledTimes(2);

    fireEvent.keyDown(strip, { key: " " });
    expect(onToggle).toHaveBeenCalledTimes(3);

    fireEvent.keyDown(strip, { key: "Tab" });
    expect(onToggle).toHaveBeenCalledTimes(3);

    fireEvent.mouseEnter(strip);
    expect(strip.style.backgroundColor).toBe("var(--overlay-weak)");

    fireEvent.mouseLeave(strip);
    expect(strip.style.backgroundColor).toBe("var(--app-sidebar-bg)");
  });

  it("renders the overlay backdrop and floating sidebar when overlay is open", () => {
    const onClose = vi.fn();
    render(
      <AgentsShellLayout
        {...buildProps({
          isSidebarOverlayOpen: true,
          onCloseSidebarOverlay: onClose,
        })}
      />
    );

    const backdrop = screen.getByTestId("agents-sidebar-overlay-backdrop");
    expect(backdrop).toBeInTheDocument();

    fireEvent.click(backdrop);
    expect(onClose).toHaveBeenCalledTimes(1);

    expect(
      screen.queryByTestId("agents-sidebar-container")
    ).not.toBeInTheDocument();

    expect(screen.getByTestId("mock-agents-sidebar")).toBeInTheDocument();
  });
});
