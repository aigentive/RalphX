import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ErrorBoundary } from "./ErrorBoundary";

function ThrowError({ error }: { error: Error }): never {
  throw error;
}

function ThrowingChild() {
  throw new Error("Production failure details");
}

describe("ErrorBoundary", () => {
  const reloadMock = vi.fn();

  beforeEach(() => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    vi.stubGlobal("location", { reload: reloadMock });
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("renders the error message and collapsible details in the production fallback", () => {
    vi.stubEnv("DEV", false);

    render(
      <ErrorBoundary>
        <ThrowingChild />
      </ErrorBoundary>,
    );

    expect(screen.getByText("Production failure details")).toBeInTheDocument();
    expect(screen.getByText("Error details")).toBeInTheDocument();

    vi.unstubAllEnvs();
  });

  it("persists the caught error and component stack through Tauri", async () => {
    render(
      <ErrorBoundary>
        <ThrowingChild />
      </ErrorBoundary>,
    );

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("log_frontend_error", {
        input: {
          message: "Production failure details",
          componentStack: expect.stringContaining("ThrowingChild"),
        },
      });
    });
  });

  it("keeps rendering the fallback when Tauri logging is unavailable", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("Tauri is unavailable"));

    render(
      <ErrorBoundary>
        <ThrowingChild />
      </ErrorBoundary>,
    );

    expect(screen.getByText("Production failure details")).toBeInTheDocument();
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("log_frontend_error", {
        input: expect.objectContaining({
          message: "Production failure details",
        }),
      });
    });
  });

  it("shows Reload and reloads when a module fails to load", () => {
    render(
      <ErrorBoundary>
        <ThrowError
          error={new TypeError("Importing a module script failed.")}
        />
      </ErrorBoundary>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Reload" }));

    expect(reloadMock).toHaveBeenCalledOnce();
  });

  it("shows only Try Again for a generic error", () => {
    render(
      <ErrorBoundary>
        <ThrowError error={new Error("Something else failed")} />
      </ErrorBoundary>,
    );

    expect(
      screen.queryByRole("button", { name: "Reload" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Try Again" }),
    ).toBeInTheDocument();
  });

  it("clears the error state and re-renders children when Try Again is clicked", () => {
    let shouldThrow = true;
    function ThrowUntilReset() {
      if (shouldThrow) {
        throw new Error("Temporary failure");
      }

      return <p>Recovered content</p>;
    }

    render(
      <ErrorBoundary>
        <ThrowUntilReset />
      </ErrorBoundary>,
    );

    shouldThrow = false;
    fireEvent.click(screen.getByRole("button", { name: "Try Again" }));

    expect(screen.getByText("Recovered content")).toBeInTheDocument();
  });

  it("renders a custom fallback instead of either built-in error branch", () => {
    render(
      <ErrorBoundary fallback={<p>Custom recovery</p>}>
        <ThrowError
          error={new TypeError("Importing a module script failed.")}
        />
      </ErrorBoundary>,
    );

    expect(screen.getByText("Custom recovery")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Reload" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Try Again" }),
    ).not.toBeInTheDocument();
  });
});
