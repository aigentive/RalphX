/**
 * The mechanism test for soft-disabled menu items.
 *
 * The defect this guards: `ContextMenuItem` / `DropdownMenuItem` style disabled items
 * with `data-[disabled]:pointer-events-none`, so a native `title` tooltip on a disabled
 * item can never be hovered, and Radix drops disabled items out of the roving-focus
 * order so it cannot be reached by keyboard either. The assertions below are therefore
 * paired: the explanation must be reachable BOTH ways, and the item must still refuse
 * to activate on every path (mouse click, Enter, Space).
 */

import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "./context-menu";
import { TooltipProvider } from "./tooltip";
import {
  EXPLAINED_DISABLED_MENU_ITEM_CLASS,
  MenuItemExplanation,
  explainedDisabledMenuItemProps,
} from "./menu-item-explanation";

const REASON = "Agent control is off for this device — enable it on the host.";

function Harness({
  reason,
  onSelect,
}: {
  reason: string | null;
  onSelect: () => void;
}) {
  const softDisabled = reason !== null;
  return (
    <TooltipProvider delayDuration={0}>
      <ContextMenu>
        <ContextMenuTrigger asChild>
          <div data-testid="trigger">Trigger</div>
        </ContextMenuTrigger>
        <ContextMenuContent>
          <ContextMenuItem data-testid="first-item">First</ContextMenuItem>
          <MenuItemExplanation reason={reason} testId="explanation">
            <ContextMenuItem
              data-testid="gated-item"
              onClick={softDisabled ? undefined : onSelect}
              className={softDisabled ? EXPLAINED_DISABLED_MENU_ITEM_CLASS : undefined}
              {...(softDisabled ? explainedDisabledMenuItemProps() : {})}
            >
              Resume
            </ContextMenuItem>
          </MenuItemExplanation>
        </ContextMenuContent>
      </ContextMenu>
    </TooltipProvider>
  );
}

function openMenu() {
  fireEvent.contextMenu(screen.getByTestId("trigger"));
}

describe("MenuItemExplanation", () => {
  it("renders the item untouched when there is no reason", () => {
    const onSelect = vi.fn();
    render(<Harness reason={null} onSelect={onSelect} />);
    openMenu();

    const item = screen.getByTestId("gated-item");
    expect(item).not.toHaveAttribute("aria-disabled");
    expect(item).not.toHaveAttribute("data-disabled-explained");

    fireEvent.click(item);
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("reveals the explanation on hover — the path a native title cannot serve", async () => {
    const user = userEvent.setup();
    render(<Harness reason={REASON} onSelect={vi.fn()} />);
    openMenu();

    await user.hover(screen.getByTestId("gated-item"));

    expect(await screen.findByTestId("explanation")).toHaveTextContent(REASON);
  });

  it("reveals the explanation on keyboard focus", async () => {
    render(<Harness reason={REASON} onSelect={vi.fn()} />);
    openMenu();

    // Radix moves menu focus programmatically with the arrow keys; focusing the item is
    // the same event the roving-focus group produces.
    screen.getByTestId("gated-item").focus();

    await waitFor(() => {
      expect(screen.getByTestId("explanation")).toHaveTextContent(REASON);
    });
  });

  it("keeps the item hoverable and focusable instead of Radix-disabled", () => {
    render(<Harness reason={REASON} onSelect={vi.fn()} />);
    openMenu();

    const item = screen.getByTestId("gated-item");
    // `data-disabled` is what triggers `pointer-events: none` and what removes the item
    // from the roving-focus order. Its absence is the whole fix.
    expect(item).not.toHaveAttribute("data-disabled");
    expect(item).toHaveAttribute("aria-disabled", "true");
    expect(item.className).toContain(EXPLAINED_DISABLED_MENU_ITEM_CLASS);
  });

  it("stays in the menu's arrow-key focus order", async () => {
    render(<Harness reason={REASON} onSelect={vi.fn()} />);
    openMenu();

    const menu = screen.getByRole("menu");
    // A Radix-`disabled` item is skipped by the roving-focus group, so arrow navigation
    // would land past it and its explanation would never be keyboard-reachable.
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    await waitFor(() => {
      expect(document.activeElement).toBe(screen.getByTestId("first-item"));
    });

    fireEvent.keyDown(screen.getByTestId("first-item"), { key: "ArrowDown" });
    await waitFor(() => {
      expect(document.activeElement).toBe(screen.getByTestId("gated-item"));
    });
  });

  it("never activates on click", () => {
    const onSelect = vi.fn();
    render(<Harness reason={REASON} onSelect={onSelect} />);
    openMenu();

    fireEvent.click(screen.getByTestId("gated-item"));

    expect(onSelect).not.toHaveBeenCalled();
    // The menu stays open, so the explanation remains readable.
    expect(screen.getByTestId("gated-item")).toBeInTheDocument();
  });

  it("never activates on Enter or Space", () => {
    const onSelect = vi.fn();
    render(<Harness reason={REASON} onSelect={onSelect} />);
    openMenu();

    const item = screen.getByTestId("gated-item");
    item.focus();
    fireEvent.keyDown(item, { key: "Enter" });
    fireEvent.keyDown(item, { key: " " });

    expect(onSelect).not.toHaveBeenCalled();
  });
});
