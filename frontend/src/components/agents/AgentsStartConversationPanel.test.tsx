import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { toast } from "sonner";
import { PERSONA_UNAVAILABLE_PREFIX } from "@/lib/personaErrors";
import { MCP_SETUP_PREFLIGHT_MARKER } from "./agentStartErrors";
import { AgentsStartConversationPanel } from "./AgentsStartConversationPanel";

vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));
vi.mock("./AgentsStartComposer", () => ({
  AgentsStartComposer: ({ onSubmit }: { onSubmit: () => Promise<void> }) => (
    <button type="button" onClick={() => void onSubmit().catch(() => {})}>Start</button>
  ),
}));

describe("AgentsStartConversationPanel", () => {
  it("does not toast persona-unavailable start errors", async () => {
    render(
      <AgentsStartConversationPanel
        defaultProjectId={null}
        defaultRuntime={null}
        isLoadingProjects={false}
        modelRegistry={{ claude: [], codex: [] }}
        onStartAgentConversation={async () => {
          throw new Error(`${PERSONA_UNAVAILABLE_PREFIX} reviewer was archived]`);
        }}
        projects={[]}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    await waitFor(() => expect(toast.error).not.toHaveBeenCalled());
  });

  it("does not toast handled MCP setup preflight errors", async () => {
    render(
      <AgentsStartConversationPanel
        defaultProjectId={null}
        defaultRuntime={null}
        isLoadingProjects={false}
        modelRegistry={{ claude: [], codex: [] }}
        onCreateProject={vi.fn()}
        onStartAgentConversation={async () => {
          throw new Error(
            `${MCP_SETUP_PREFLIGHT_MARKER}{"provider":"claude","server_id":"ralphx","scope":"user","conflict_kind":"ambiguous_reserved_id","repair_status":"manual_only"}`,
          );
        }}
        projects={[]}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    await waitFor(() => expect(toast.error).not.toHaveBeenCalled());
  });
});
