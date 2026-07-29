import { render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ErrorBoundary } from "./ErrorBoundary";

function ThrowingChild() {
  throw new Error("Production failure details");
}

describe("ErrorBoundary", () => {
  beforeEach(() => {
    vi.stubEnv("DEV", false);
    vi.spyOn(console, "error").mockImplementation(() => undefined);
  });

  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it("renders the error message and collapsible details in the production fallback", () => {
    render(
      <ErrorBoundary>
        <ThrowingChild />
      </ErrorBoundary>,
    );

    expect(screen.getByText("Production failure details")).toBeInTheDocument();
    expect(screen.getByText("Error details")).toBeInTheDocument();
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
        input: expect.objectContaining({ message: "Production failure details" }),
      });
    });
  });
});
