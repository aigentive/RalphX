import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { GitBranch } from "lucide-react";

import { TooltipProvider } from "@/components/ui/tooltip";
import { PublishFact } from "./AgentsPublishFact";

function withTooltip(node: React.ReactNode) {
  return <TooltipProvider delayDuration={0}>{node}</TooltipProvider>;
}

describe("PublishFact", () => {
  it("renders label, value and description", () => {
    render(
      withTooltip(
        <PublishFact
          icon={GitBranch}
          label="Branch"
          value="feature/polish"
          description="3 commits ahead of main"
        />,
      ),
    );
    expect(screen.getByText("Branch")).toBeInTheDocument();
    expect(screen.getByText("feature/polish")).toBeInTheDocument();
    expect(screen.getByText("3 commits ahead of main")).toBeInTheDocument();
  });

  it("invokes the action callback when the icon button is clicked", async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();
    render(
      withTooltip(
        <PublishFact
          icon={GitBranch}
          label="Branch"
          value="feature/polish"
          action={{ label: "Open in browser", testId: "fact-action", onClick }}
        />,
      ),
    );
    await user.click(screen.getByTestId("fact-action"));
    expect(onClick).toHaveBeenCalledOnce();
  });

  it("invokes descriptionAction callback when the description button is clicked", async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();
    render(
      withTooltip(
        <PublishFact
          icon={GitBranch}
          label="PR"
          value="#42"
          description="Open PR"
          descriptionAction={{
            label: "Open PR description",
            testId: "fact-desc-action",
            onClick,
          }}
        />,
      ),
    );
    await user.click(screen.getByTestId("fact-desc-action"));
    expect(onClick).toHaveBeenCalledOnce();
  });
});
