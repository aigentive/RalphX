import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { PullRequestDetail } from "@/api/github";
import { openExternalTicketUrl } from "@/components/ticketing/ticketing-open-external";
import { TooltipProvider } from "@/components/ui/tooltip";

import { AgentsPublishChecksTab } from "./AgentsPublishChecksTab";

vi.mock("@/components/ticketing/ticketing-open-external", () => ({
  openExternalTicketUrl: vi.fn(),
}));

function detail(
  overrides: Partial<PullRequestDetail> = {},
): PullRequestDetail {
  return {
    state: "loaded",
    origin: "ownedOutbound",
    description: null,
    checks: [],
    reviewSummary: null,
    issueComments: [],
    reviewThread: [],
    rxConversations: [],
    linkedTickets: [],
    sourcesUnavailable: [],
    ...overrides,
  };
}

function renderTab(
  props: Partial<Parameters<typeof AgentsPublishChecksTab>[0]> = {},
) {
  return render(
    <TooltipProvider delayDuration={0}>
      <AgentsPublishChecksTab
        detail={null}
        isError={false}
        isLoading={false}
        isReady
        {...props}
      />
    </TooltipProvider>,
  );
}

describe("AgentsPublishChecksTab", () => {
  it("paints a lightweight shell before hydration is ready", () => {
    renderTab({ isReady: false });

    const shell = screen.getByTestId("agents-publish-checks-shell");
    expect(shell).toBeInTheDocument();
    expect(shell.style.backgroundColor).toBe("var(--bg-subtle)");
    expect(shell.style.borderColor).toBe("var(--border-subtle)");
    expect(shell.style.borderStyle).toBe("solid");
    expect(shell.style.borderWidth).toBe("1px");
    expect(screen.queryByText("Loading checks…")).not.toBeInTheDocument();
    expect(
      screen.queryByText("Checks are unavailable right now."),
    ).not.toBeInTheDocument();
  });

  it("distinguishes loading, unavailable, and empty states", () => {
    const result = renderTab({ isLoading: true });
    expect(screen.getByText("Loading checks…")).toBeInTheDocument();

    result.rerender(
      <TooltipProvider delayDuration={0}>
        <AgentsPublishChecksTab
          detail={null}
          isError
          isLoading={false}
          isReady
        />
      </TooltipProvider>,
    );
    expect(screen.getByText("Checks are unavailable right now.")).toBeInTheDocument();
    expect(
      screen.queryByText("No checks reported for this PR yet."),
    ).not.toBeInTheDocument();

    result.rerender(
      <TooltipProvider delayDuration={0}>
        <AgentsPublishChecksTab
          detail={detail()}
          isError={false}
          isLoading={false}
          isReady
        />
      </TooltipProvider>,
    );
    expect(
      screen.getByText("No checks reported for this PR yet."),
    ).toBeInTheDocument();
  });

  it("treats an unavailable checks source as unavailable instead of empty", () => {
    renderTab({
      detail: detail({ sourcesUnavailable: ["checks"] }),
    });

    expect(screen.getByText("Checks are unavailable right now.")).toBeInTheDocument();
    expect(
      screen.queryByText("No checks reported for this PR yet."),
    ).not.toBeInTheDocument();
  });

  it("renders all shared check rows and opens URL-backed details", async () => {
    const user = userEvent.setup();
    renderTab({
      detail: detail({
        checks: [
          {
            name: "lint",
            status: "completed",
            conclusion: "failure",
            detailsUrl: "https://github.com/acme/app/actions/runs/1",
          },
          {
            name: "types",
            status: "completed",
            conclusion: "success",
            detailsUrl: null,
          },
        ],
      }),
    });

    expect(screen.getByText("lint")).toBeInTheDocument();
    expect(screen.getByText("types")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Open types check details" }),
    ).not.toBeInTheDocument();

    const detailsButton = screen.getByRole("button", {
      name: "Open lint check details",
    });
    detailsButton.focus();
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "Open lint check details",
    );

    await user.click(detailsButton);
    expect(openExternalTicketUrl).toHaveBeenCalledWith(
      "https://github.com/acme/app/actions/runs/1",
    );
  });
});
