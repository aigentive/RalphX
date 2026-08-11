import { FileText, Ticket } from "lucide-react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";

import { AgentsArtifactTabCustomizer } from "./AgentsArtifactTabCustomizer";

describe("AgentsArtifactTabCustomizer", () => {
  it("groups shown, hidden, and unavailable tabs with conversation scope guidance", async () => {
    const user = userEvent.setup();
    const onHide = vi.fn();
    const onShow = vi.fn();

    render(
      <TooltipProvider delayDuration={0}>
        <AgentsArtifactTabCustomizer
          tabs={[
            { id: "plan", label: "Plan", icon: FileText, available: true },
            { id: "jira", label: "Jira", icon: Ticket, available: true },
            {
              id: "review",
              label: "Review",
              icon: FileText,
              available: false,
              unavailableReason: "Appears when a review is created.",
            },
          ]}
          hiddenTabs={["jira"]}
          onHide={onHide}
          onShow={onShow}
        />
      </TooltipProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Customize tabs" }));

    expect(screen.getByText("Shown")).toBeInTheDocument();
    expect(screen.getByText("Hidden")).toBeInTheDocument();
    expect(screen.getByText("Not available in this conversation")).toBeInTheDocument();
    expect(screen.getByText("Appears when a review is created.")).toBeInTheDocument();
    expect(screen.getByText("Applies to this conversation.")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Hide Plan" }));
    expect(onHide).toHaveBeenCalledWith("plan");

    await user.click(screen.getByRole("button", { name: "Show Jira" }));
    expect(onShow).toHaveBeenCalledWith("jira");
  });

  it("opens synchronously from the full empty-state trigger", async () => {
    const user = userEvent.setup();

    render(
      <TooltipProvider delayDuration={0}>
        <AgentsArtifactTabCustomizer
          triggerVariant="button"
          tabs={[
            { id: "plan", label: "Plan", icon: FileText, available: true },
          ]}
          hiddenTabs={["plan"]}
          onHide={vi.fn()}
          onShow={vi.fn()}
        />
      </TooltipProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Customize tabs" }));
    expect(screen.getByRole("dialog", { name: "Customize artifact tabs" })).toBeVisible();
  });
});
