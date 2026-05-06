import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { PlanClearAction } from "./PlanClearAction";

describe("PlanClearAction", () => {
  it("renders the Clear active plan label and routes click + hover", async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();
    const onMouseEnter = vi.fn();
    render(
      <PlanClearAction isHighlighted={false} onClick={onClick} onMouseEnter={onMouseEnter} />,
    );
    expect(screen.getByText("Clear active plan")).toBeInTheDocument();
    expect(screen.getByText("Return to no active plan state")).toBeInTheDocument();

    await user.hover(screen.getByTestId("plan-quick-switcher-clear"));
    expect(onMouseEnter).toHaveBeenCalled();

    await user.click(screen.getByTestId("plan-quick-switcher-clear"));
    expect(onClick).toHaveBeenCalledOnce();
  });

  it("paints the highlighted background when isHighlighted=true", () => {
    render(
      <PlanClearAction isHighlighted onClick={vi.fn()} onMouseEnter={vi.fn()} />,
    );
    const btn = screen.getByTestId("plan-quick-switcher-clear");
    expect(btn.style.background).not.toBe("transparent");
    expect(btn.style.border).toContain("var(--accent-border)");
  });
});
