/* eslint-disable react-refresh/only-export-components */
/**
 * Explanations for menu items the user is not allowed to activate.
 *
 * ## Why a native `title` does not work here
 *
 * `ContextMenuItem` / `DropdownMenuItem` both carry `data-[disabled]:pointer-events-none`,
 * and Radix stamps `data-disabled` on any item rendered with `disabled`. A `title`
 * tooltip is a native HOVER affordance, so an element that receives no pointer events
 * can never show one. Radix additionally drops `disabled` items out of the menu's
 * roving focus, so the explanation is unreachable by keyboard too. The result is a
 * greyed-out action with no discoverable reason — exactly the state the gate copy
 * exists to prevent.
 *
 * ## The mechanism
 *
 * The item stays ENABLED as far as Radix is concerned (so it keeps pointer events and
 * stays in the roving-focus order) and is soft-disabled instead:
 *
 * - `aria-disabled` — assistive tech announces it as unavailable.
 * - `onClick` + `onSelect` both `preventDefault()` — activation is blocked on the mouse
 *   path, the Enter/Space path (Radix synthesizes a click), and the select path. Radix's
 *   `composeEventHandlers` skips its own `handleSelect` once our `onClick` has prevented
 *   default, so no handler runs and the menu does not close.
 * - `MenuItemExplanation` wraps the item in the app tooltip component, which opens on
 *   hover AND on focus — so the reason is reachable with mouse and keyboard alike.
 *
 * Callers must still omit the real action handler when soft-disabling; the props below
 * are the second brake, not the first. See `.claude/rules/icon-only-buttons.md` (root
 * CLAUDE.md rule 23): an affordance needs an accessible name AND the app tooltip
 * component — native `title` alone is not enough.
 */

import type { MouseEvent, ReactNode } from "react";

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

/** Matches the `data-[disabled]:opacity-50` dimming of a genuinely disabled item. */
export const EXPLAINED_DISABLED_MENU_ITEM_CLASS = "opacity-50";

export interface ExplainedDisabledMenuItemProps {
  readonly "aria-disabled": true;
  /** Styling/query hook standing in for Radix's `data-disabled`. */
  readonly "data-disabled-explained": "true";
  readonly onSelect: (event: Event) => void;
  readonly onClick: (event: MouseEvent) => void;
}

/**
 * Props that make a menu item inert while keeping it hoverable and focusable.
 *
 * Spread onto the item LAST so the blocking handlers win over any action handler the
 * call site passed.
 */
export function explainedDisabledMenuItemProps(): ExplainedDisabledMenuItemProps {
  return {
    "aria-disabled": true,
    "data-disabled-explained": "true",
    onSelect: (event: Event) => {
      event.preventDefault();
    },
    onClick: (event: MouseEvent) => {
      event.preventDefault();
      event.stopPropagation();
    },
  };
}

export interface MenuItemExplanationProps {
  /** The explanation, or `null` when the item needs none (renders children untouched). */
  reason: string | null;
  children: ReactNode;
  testId?: string;
}

/**
 * Attaches the app tooltip to a menu item so its disabled reason is discoverable.
 *
 * Renders `children` unchanged when there is no reason, so an enabled item pays no
 * tooltip cost. Requires a `TooltipProvider` ancestor (the app root has one).
 */
export function MenuItemExplanation({
  reason,
  children,
  testId,
}: MenuItemExplanationProps) {
  if (reason === null || reason === "") {
    return <>{children}</>;
  }

  return (
    <Tooltip>
      <TooltipTrigger asChild>{children}</TooltipTrigger>
      <TooltipContent
        side="right"
        className="max-w-[280px] text-xs"
        data-testid={testId ?? "menu-item-explanation"}
      >
        {reason}
      </TooltipContent>
    </Tooltip>
  );
}
