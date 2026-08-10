/**
 * Tests for PlanEditor component
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PlanEditor } from "./PlanEditor";
import type { Artifact } from "@/types/artifact";

vi.mock("@/api/backend", () => ({
  backendApiUrl: (endpoint: string) => `http://backend.test/api/${endpoint}`,
  // PR 2.2: the component now goes through the environment-aware seam. The double
  // keeps forwarding to the global fetch mock so the assertions below still describe
  // the request that leaves the app.
  backendFetch: (endpoint: string, ...init: [init?: RequestInit]) =>
    fetch(`http://backend.test/api/${endpoint}`, ...init),
}));

// Mock fetch
const mockFetch = vi.fn();
global.fetch = mockFetch;

// Mock window.confirm
const mockConfirm = vi.fn();
global.confirm = mockConfirm;

describe("PlanEditor", () => {
  const mockPlan: Artifact = {
    id: "plan-1",
    type: "Specification",
    name: "Test Plan",
    content: {
      type: "inline",
      text: "# Original Content\n\nThis is the original plan content.",
    },
    metadata: {
      createdAt: "2024-01-01T00:00:00Z",
      createdBy: "orchestrator",
      version: 1,
    },
    derivedFrom: [],
  };

  const mockOnSave = vi.fn();
  const mockOnCancel = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    mockFetch.mockResolvedValue({
      ok: true,
      json: async () => ({
        id: "plan-1",
        name: "Test Plan",
        artifact_type: "Specification",
        content_type: "inline",
        content: "# Updated Content",
        created_at: "2024-01-01T00:00:00Z",
        created_by: "orchestrator",
        version: 2,
        bucket_id: "prd-library",
        task_id: null,
        process_id: null,
        derived_from: [],
      }),
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders with plan name and content", () => {
    render(
      <PlanEditor plan={mockPlan} onSave={mockOnSave} onCancel={mockOnCancel} />
    );

    expect(screen.getByText(/Edit Plan: Test Plan/)).toBeInTheDocument();
    expect(screen.getByDisplayValue(/Original Content/)).toBeInTheDocument();
  });

  it("toggles between edit and preview mode", async () => {
    const user = userEvent.setup();
    render(
      <PlanEditor plan={mockPlan} onSave={mockOnSave} onCancel={mockOnCancel} />
    );

    // Initially in edit mode
    expect(screen.getByRole("textbox")).toBeInTheDocument();
    expect(screen.getByText("Preview")).toBeInTheDocument();

    // Click preview button
    await user.click(screen.getByText("Preview"));

    // Now in preview mode
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(screen.getByText("Edit")).toBeInTheDocument();
    expect(screen.getByText("Original Content")).toBeInTheDocument();

    // Click edit button
    await user.click(screen.getByText("Edit"));

    // Back in edit mode
    expect(screen.getByRole("textbox")).toBeInTheDocument();
  });

  it("allows editing content", async () => {
    const user = userEvent.setup();
    render(
      <PlanEditor plan={mockPlan} onSave={mockOnSave} onCancel={mockOnCancel} />
    );

    const textarea = screen.getByRole("textbox");

    // Clear and type new content
    await user.clear(textarea);
    await user.type(textarea, "# New Content");

    expect(textarea).toHaveValue("# New Content");
    expect(screen.getByText("You have unsaved changes")).toBeInTheDocument();
  });

  it("saves changes when save button is clicked", async () => {
    const user = userEvent.setup();
    render(
      <PlanEditor plan={mockPlan} onSave={mockOnSave} onCancel={mockOnCancel} />
    );

    const textarea = screen.getByRole("textbox");

    // Edit content
    await user.clear(textarea);
    await user.type(textarea, "# Updated Content");

    // Click save button
    const saveButton = screen.getByText("Save");
    await user.click(saveButton);

    // Should call fetch with correct endpoint and data
    await waitFor(() => {
      expect(mockFetch).toHaveBeenCalledWith(
        "http://backend.test/api/update_plan_artifact",
        expect.objectContaining({
          method: "POST",
          headers: {
            "Content-Type": "application/json",
          },
          body: JSON.stringify({
            artifact_id: "plan-1",
            content: "# Updated Content",
          }),
        })
      );
    });

    // Should call onSave with updated plan
    await waitFor(() => {
      expect(mockOnSave).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "plan-1",
          content: {
            type: "inline",
            text: "# Updated Content",
          },
          metadata: expect.objectContaining({
            version: 2,
          }),
        })
      );
    });
  });

  it("disables save button when no changes", () => {
    render(
      <PlanEditor plan={mockPlan} onSave={mockOnSave} onCancel={mockOnCancel} />
    );

    const saveButton = screen.getByText("Save");
    expect(saveButton).toBeDisabled();
  });

  it("shows error message when save fails", async () => {
    const user = userEvent.setup();
    mockFetch.mockResolvedValueOnce({
      ok: false,
      statusText: "Internal Server Error",
    });

    render(
      <PlanEditor plan={mockPlan} onSave={mockOnSave} onCancel={mockOnCancel} />
    );

    const textarea = screen.getByRole("textbox");
    await user.clear(textarea);
    await user.type(textarea, "# New Content");

    const saveButton = screen.getByText("Save");
    await user.click(saveButton);

    await waitFor(() => {
      expect(
        screen.getByText(/Failed to update plan: Internal Server Error/)
      ).toBeInTheDocument();
    });

    expect(mockOnSave).not.toHaveBeenCalled();
  });

  it("prompts confirmation when canceling with unsaved changes", async () => {
    const user = userEvent.setup();
    mockConfirm.mockReturnValue(false); // User says no

    render(
      <PlanEditor plan={mockPlan} onSave={mockOnSave} onCancel={mockOnCancel} />
    );

    const textarea = screen.getByRole("textbox");
    await user.clear(textarea);
    await user.type(textarea, "# New Content");

    const cancelButton = screen.getByText("Cancel");
    await user.click(cancelButton);

    expect(mockConfirm).toHaveBeenCalledWith(
      "You have unsaved changes. Are you sure you want to cancel?"
    );
    expect(mockOnCancel).not.toHaveBeenCalled();
  });

  it("cancels without confirmation when no changes", async () => {
    const user = userEvent.setup();

    render(
      <PlanEditor plan={mockPlan} onSave={mockOnSave} onCancel={mockOnCancel} />
    );

    const cancelButton = screen.getByText("Cancel");
    await user.click(cancelButton);

    expect(mockConfirm).not.toHaveBeenCalled();
    expect(mockOnCancel).toHaveBeenCalled();
  });

  it("confirms and cancels when user confirms with unsaved changes", async () => {
    const user = userEvent.setup();
    mockConfirm.mockReturnValue(true); // User says yes

    render(
      <PlanEditor plan={mockPlan} onSave={mockOnSave} onCancel={mockOnCancel} />
    );

    const textarea = screen.getByRole("textbox");
    await user.clear(textarea);
    await user.type(textarea, "# New Content");

    const cancelButton = screen.getByText("Cancel");
    await user.click(cancelButton);

    expect(mockConfirm).toHaveBeenCalled();
    expect(mockOnCancel).toHaveBeenCalled();
  });

  it("disables inputs while saving", async () => {
    const user = userEvent.setup();
    // Make fetch slow
    mockFetch.mockImplementation(
      () =>
        new Promise((resolve) =>
          setTimeout(
            () =>
              resolve({
                ok: true,
                json: async () => ({
                  id: "plan-1",
                  name: "Test Plan",
                  artifact_type: "Specification",
                  content_type: "inline",
                  content: "# Updated Content",
                  created_at: "2024-01-01T00:00:00Z",
                  created_by: "orchestrator",
                  version: 2,
                  bucket_id: "prd-library",
                  task_id: null,
                  process_id: null,
                  derived_from: [],
                }),
              }),
            100
          )
        )
    );

    render(
      <PlanEditor plan={mockPlan} onSave={mockOnSave} onCancel={mockOnCancel} />
    );

    const textarea = screen.getByRole("textbox");
    await user.clear(textarea);
    await user.type(textarea, "# New Content");

    const saveButton = screen.getByText("Save");
    await user.click(saveButton);

    // While saving
    expect(screen.getByText("Saving...")).toBeInTheDocument();
    expect(textarea).toBeDisabled();
    expect(screen.getByText("Cancel")).toBeDisabled();

    // Wait for save to complete
    await waitFor(() => {
      expect(mockOnSave).toHaveBeenCalled();
    });
  });
});
