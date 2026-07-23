import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { NoticeBanner } from "./notice-banner";

describe("NoticeBanner", () => {
  it.each([
    ["warning", "var(--status-warning-muted, rgba(224, 179, 65, 0.1))"],
    ["error", "var(--status-error-muted, rgba(213, 94, 0, 0.1))"],
    ["success", "var(--status-success-muted, rgba(63, 191, 127, 0.1))"],
    ["neutral", "var(--bg-surface, #1e1e23)"],
    ["accent", "var(--accent-muted, rgba(255, 106, 53, 0.1))"],
  ] as const)("maps %s to its semantic tinted surface", (tone, backgroundColor) => {
    render(
      <NoticeBanner tone={tone} title="Notice" testId="notice">
        Detail
      </NoticeBanner>,
    );

    const notice = screen.getByTestId("notice");
    expect(notice).toHaveAttribute("data-tone", tone);
    expect(notice.style.backgroundColor).toBe(backgroundColor);
  });

  it("renders icon, title, body, and action with WKWebView-safe longhands", () => {
    render(
      <NoticeBanner
        tone="error"
        icon={<svg data-testid="notice-icon" />}
        title="Failed"
        action={<button type="button">Retry</button>}
        testId="notice"
      >
        Publish failed.
      </NoticeBanner>,
    );

    const notice = screen.getByTestId("notice");
    expect(screen.getByTestId("notice-icon")).toBeInTheDocument();
    expect(notice).toHaveTextContent("Failed");
    expect(notice).toHaveTextContent("Publish failed.");
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
    expect(notice.style.backgroundColor).not.toBe("");
    expect(notice.style.borderColor).not.toBe("");
    expect(notice.style.borderStyle).toBe("solid");
    expect(notice.style.borderWidth).toBe("1px");
  });
});
