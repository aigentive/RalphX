import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useAgentModels } from "@/hooks/useAgentModels";

import { AgentModelsSection } from "./AgentModelsSection";

vi.mock("@/hooks/useAgentModels", () => ({
  useAgentModels: vi.fn(),
}));

const upsertModelAsync = vi.fn();
const deleteModelAsync = vi.fn();

function mockUseAgentModels() {
  vi.mocked(useAgentModels).mockReturnValue({
    models: [
      {
        provider: "codex",
        modelId: "gpt-5.5",
        label: "gpt-5.5",
        menuLabel: "gpt-5.5 (Current)",
        description: "Frontier model.",
        supportedEfforts: ["low", "medium", "high", "xhigh"],
        defaultEffort: "xhigh",
        source: "built_in",
        enabled: true,
        createdAt: null,
        updatedAt: null,
      },
      {
        provider: "codex",
        modelId: "gpt-5.6",
        label: "gpt-5.6",
        menuLabel: "gpt-5.6",
        description: null,
        supportedEfforts: ["low", "medium", "high"],
        defaultEffort: "high",
        source: "custom",
        enabled: true,
        createdAt: null,
        updatedAt: null,
      },
    ],
    registry: {
      claude: [],
      codex: [],
    },
    isLoading: false,
    isPlaceholderData: false,
    isError: false,
    error: null,
    upsertModel: vi.fn(),
    upsertModelAsync,
    isUpserting: false,
    upsertError: null,
    deleteModel: vi.fn(),
    deleteModelAsync,
    isDeleting: false,
    deleteError: null,
  });
}

describe("AgentModelsSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    upsertModelAsync.mockResolvedValue({});
    deleteModelAsync.mockResolvedValue(true);
    mockUseAgentModels();
  });

  it("shows built-in and custom models", () => {
    render(<AgentModelsSection />);

    expect(screen.getByText("gpt-5.5 (Current)")).toBeInTheDocument();
    expect(screen.getByText("Built-in")).toBeInTheDocument();
    expect(screen.getAllByText("gpt-5.6").length).toBeGreaterThan(0);
    expect(screen.getByText("Custom")).toBeInTheDocument();
  });

  it("saves a custom model with effort compatibility", async () => {
    render(<AgentModelsSection />);

    fireEvent.change(screen.getByLabelText("Model ID"), {
      target: { value: "gpt-5.7" },
    });
    fireEvent.change(screen.getByLabelText("Label"), {
      target: { value: "GPT 5.7" },
    });
    fireEvent.change(screen.getByLabelText("Menu Label"), {
      target: { value: "gpt-5.7" },
    });
    fireEvent.click(screen.getByRole("button", { name: /save model/i }));

    await waitFor(() =>
      expect(upsertModelAsync).toHaveBeenCalledWith(
        expect.objectContaining({
          provider: "codex",
          modelId: "gpt-5.7",
          label: "GPT 5.7",
          menuLabel: "gpt-5.7",
          supportedEfforts: ["low", "medium", "high", "xhigh"],
          defaultEffort: "xhigh",
        }),
      ),
    );
  });

  it("validates model id and effort selection before saving", async () => {
    render(<AgentModelsSection />);

    fireEvent.click(screen.getByRole("button", { name: /save model/i }));
    expect(screen.getByText("Model ID is required")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Model ID"), {
      target: { value: "gpt-5.8" },
    });
    for (const label of ["Low", "Medium", "High", "Extra High"]) {
      fireEvent.click(screen.getByRole("checkbox", { name: label }));
    }
    fireEvent.click(screen.getByRole("button", { name: /save model/i }));

    expect(screen.getByText("Select at least one effort")).toBeInTheDocument();
    expect(upsertModelAsync).not.toHaveBeenCalled();
  });

  it("saves fallback labels, nullable description, and disabled state", async () => {
    render(<AgentModelsSection />);

    fireEvent.change(screen.getByLabelText("Model ID"), {
      target: { value: "  gpt-5.8  " },
    });
    fireEvent.change(screen.getByLabelText("Description"), {
      target: { value: "   " },
    });
    fireEvent.click(screen.getByLabelText("Enabled"));
    fireEvent.click(screen.getByRole("button", { name: /save model/i }));

    await waitFor(() =>
      expect(upsertModelAsync).toHaveBeenCalledWith(
        expect.objectContaining({
          modelId: "gpt-5.8",
          label: "gpt-5.8",
          menuLabel: "gpt-5.8",
          description: null,
          enabled: false,
        }),
      ),
    );
  });

  it("can edit and delete custom models", async () => {
    render(<AgentModelsSection />);

    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    expect(screen.getByLabelText("Model ID")).toHaveValue("gpt-5.6");

    fireEvent.click(screen.getByRole("button", { name: /delete/i }));
    await waitFor(() =>
      expect(deleteModelAsync).toHaveBeenCalledWith({
        provider: "codex",
        modelId: "gpt-5.6",
      }),
    );
  });
});
