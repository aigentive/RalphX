import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ErrorBanner } from "./SettingsView.shared";

describe("Settings ErrorBanner", () => {
  it("announces errors and gives the dismiss icon an accessible tooltip", async () => {
    const user = userEvent.setup();
    const onDismiss = vi.fn();
    render(<ErrorBanner error="Could not save" onDismiss={onDismiss} />);

    expect(screen.getByRole("alert")).toHaveTextContent("Could not save");
    const dismiss = screen.getByRole("button", { name: "Dismiss error" });
    await user.hover(dismiss);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Dismiss error");

    await user.click(dismiss);
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it("does not render a no-op dismiss action for persistent load errors", () => {
    render(<ErrorBanner error="Catalog unavailable" />);

    expect(screen.getByRole("alert")).toHaveTextContent("Catalog unavailable");
    expect(screen.queryByRole("button", { name: "Dismiss error" })).not.toBeInTheDocument();
  });
});
