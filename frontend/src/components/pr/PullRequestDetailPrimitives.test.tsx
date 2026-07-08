import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { DetailSkeleton, PrMarkdown } from "./PullRequestDetailPrimitives";

describe("PrMarkdown", () => {
  it("renders GitHub details blocks as native disclosure sections", () => {
    const { container } = render(
      <PrMarkdown
        content={[
          "Before",
          "",
          "<details open>",
          "<summary>View full plan</summary>",
          "",
          "## Goal",
          "- Ship the fix",
          "</details>",
          "",
          "After",
        ].join("\n")}
      />,
    );

    const details = screen.getByTestId("pr-markdown-details");
    expect(details.tagName).toBe("DETAILS");
    expect(details).toHaveAttribute("open");
    expect(within(details).getByText("View full plan")).toBeInTheDocument();
    expect(within(details).getByRole("heading", { name: "Goal" })).toBeInTheDocument();
    expect(within(details).getByText("Ship the fix")).toBeInTheDocument();
    expect(screen.getByText("Before")).toBeInTheDocument();
    expect(screen.getByText("After")).toBeInTheDocument();
    expect(container.textContent).not.toContain("<details>");
    expect(container.textContent).not.toContain("<summary>");
  });

  it("renders GitHub details blocks collapsed by default", () => {
    render(
      <PrMarkdown
        content={[
          "<details>",
          "<summary>Collapsed by default</summary>",
          "",
          "Collapsed body",
          "</details>",
        ].join("\n")}
      />,
    );

    const details = screen.getByTestId("pr-markdown-details");
    expect(details).not.toHaveAttribute("open");
    expect(within(details).getByText("Collapsed by default")).toBeInTheDocument();
  });

  it("uses text-only summary labels when GitHub summary contains markup", () => {
    render(
      <PrMarkdown
        content={[
          "<details>",
          "<summary><strong>View</strong> full &amp; final plan<!-- hidden --></summary>",
          "",
          "Body",
          "</details>",
        ].join("\n")}
      />,
    );

    const details = screen.getByTestId("pr-markdown-details");
    expect(within(details).getByText("View full & final plan")).toBeInTheDocument();
    expect(details).not.toHaveTextContent("hidden");
    expect(details).not.toHaveTextContent("<strong>");
  });

  it("does not render unsupported raw HTML as DOM", () => {
    const { container } = render(
      <PrMarkdown content={"Before\n\n<script>alert('x')</script>\n\nAfter"} />,
    );

    expect(container.querySelector("script")).toBeNull();
    expect(screen.getByText("Before")).toBeInTheDocument();
    expect(screen.getByText("After")).toBeInTheDocument();
  });

  it("does not turn inline details literals into disclosure sections", () => {
    const { container } = render(
      <PrMarkdown
        content={
          "Inline <details><summary>Literal</summary>not a disclosure</details> text"
        }
      />,
    );

    expect(screen.queryByTestId("pr-markdown-details")).not.toBeInTheDocument();
    expect(container.textContent).toContain("<details>");
    expect(container.textContent).toContain("Literal");
  });

  it("does not turn fenced details examples into disclosure sections", () => {
    const { container } = render(
      <PrMarkdown
        content={[
          "```html",
          "<details>",
          "<summary>Example only</summary>",
          "Not a disclosure",
          "</details>",
          "```",
        ].join("\n")}
      />,
    );

    expect(screen.queryByTestId("pr-markdown-details")).not.toBeInTheDocument();
    expect(container.textContent).toContain("<details>");
    expect(container.textContent).toContain("Example only");
  });

  it("renders a details block whose body contains fenced details tag examples", () => {
    render(
      <PrMarkdown
        content={[
          "<details>",
          "<summary>View full plan</summary>",
          "",
          "Before code",
          "",
          "```html",
          "<details>",
          "<summary>Example snippet</summary>",
          "```",
          "",
          "After code",
          "</details>",
        ].join("\n")}
      />,
    );

    const details = screen.getByTestId("pr-markdown-details");
    expect(within(details).getByText("View full plan")).toBeInTheDocument();
    expect(within(details).getByText("Before code")).toBeInTheDocument();
    expect(details).toHaveTextContent("Example snippet");
    expect(within(details).getByText("After code")).toBeInTheDocument();
  });

  it("preserves malformed details markup as text", () => {
    const { container } = render(
      <PrMarkdown content={"Before\n\n<details>\nNo closing tag\n\nAfter"} />,
    );

    expect(screen.queryByTestId("pr-markdown-details")).not.toBeInTheDocument();
    expect(container.textContent).toContain("<details>");
    expect(container.textContent).toContain("No closing tag");
    expect(container.textContent).toContain("After");
  });
});

describe("DetailSkeleton", () => {
  it("uses a visible fill instead of the PR panel surface color", () => {
    render(<DetailSkeleton lines={2} />);

    const status = screen.getByRole("status", { name: "Loading pull request" });
    const lines = status.querySelectorAll("[data-testid='pr-detail-skeleton-line']");

    expect(lines).toHaveLength(2);
    expect(lines[0]).toHaveStyle("background-color: var(--bg-hover)");
  });
});
