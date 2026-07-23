import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "./tooltip";
import { CopyableRef } from "./copyable-ref";

const { toastSuccessMock, toastErrorMock } = vi.hoisted(() => ({
  toastSuccessMock: vi.fn(),
  toastErrorMock: vi.fn(),
}));

const originalClipboard = navigator.clipboard;

function installClipboard(writeText: (value: string) => Promise<void>) {
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  });
}

vi.mock("sonner", () => ({
  toast: {
    success: toastSuccessMock,
    error: toastErrorMock,
  },
}));

describe("CopyableRef", () => {
  afterEach(() => {
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    vi.restoreAllMocks();
    if (originalClipboard) {
      Object.defineProperty(navigator, "clipboard", {
        configurable: true,
        value: originalClipboard,
      });
    } else {
      delete (navigator as unknown as { clipboard?: Clipboard }).clipboard;
    }
  });

  it("renders a truncating reference with an optional prefix and accessible copy control", () => {
    render(
      <TooltipProvider>
        <CopyableRef
          value="ralphx/automation-1"
          prefixLabel="Workspace"
          ariaLabel="Copy branch"
          testId="branch"
        />
      </TooltipProvider>,
    );

    expect(screen.getByText("Workspace")).toBeInTheDocument();
    expect(screen.getByTestId("branch")).toHaveTextContent("ralphx/automation-1");
    expect(screen.getByTestId("branch-value")).toHaveClass(
      "truncate",
      "font-mono",
      "text-[0.8125rem]",
    );
    expect(screen.getByRole("button", { name: "Copy branch" })).toBeInTheDocument();
  });

  it("copies the value and reports success through a toast", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    installClipboard(writeText);
    render(
      <TooltipProvider>
        <CopyableRef value="feature/polish" ariaLabel="Copy branch" />
      </TooltipProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Copy branch" }));

    expect(writeText).toHaveBeenCalledWith("feature/polish");
    await waitFor(() => expect(toastSuccessMock).toHaveBeenCalledWith("Branch copied"));
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("reports clipboard failures", async () => {
    const user = userEvent.setup();
    installClipboard(vi.fn().mockRejectedValue(new Error("denied")));
    render(
      <TooltipProvider>
        <CopyableRef value="feature/polish" ariaLabel="Copy branch" />
      </TooltipProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Copy branch" }));

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith("Failed to copy branch"),
    );
  });
});
