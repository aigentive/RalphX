import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AgentComposerSurface } from "./AgentComposerSurface";

function renderComposer() {
  return render(
    <AgentComposerSurface
      project={{
        value: "project-1",
        onValueChange: vi.fn(),
        options: [{ id: "project-1", label: "RalphX" }],
        placeholder: "Project",
      }}
      provider={{
        value: "codex",
        onValueChange: vi.fn(),
        options: [{ id: "codex", label: "Codex" }],
      }}
      model={{
        value: "gpt-5.5",
        onValueChange: vi.fn(),
        options: [{ id: "gpt-5.5", label: "gpt-5.5 (Current)" }],
      }}
      effort={{
        value: "xhigh",
        onValueChange: vi.fn(),
        options: [{ id: "xhigh", label: "Extra High" }],
      }}
      mode={{
        value: "agent",
        onValueChange: vi.fn(),
        options: [{ id: "agent", label: "Agent" }],
      }}
      onSend={vi.fn()}
      actionTestId="agent-composer-submit"
    />
  );
}

describe("AgentComposerSurface", () => {
  it("keeps the runtime selector content-sized instead of filling the footer row", () => {
    renderComposer();

    expect(screen.getByTestId("agent-composer-runtime-pill")).toHaveClass(
      "max-w-[34rem]"
    );
    expect(screen.getByTestId("agent-composer-runtime-pill")).not.toHaveClass("flex-1");
    expect(screen.getByTestId("agent-composer-submit")).toHaveClass("ml-auto");
  });
});
