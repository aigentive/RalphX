import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { IdeationWidget } from "./IdeationWidget";
import type { ToolCall } from "./shared.constants";

function makeToolCall(name: string, overrides: Partial<ToolCall> = {}): ToolCall {
  return {
    id: "ideation-1",
    name,
    arguments: {},
    ...overrides,
  };
}

function mcpWrap(obj: unknown): unknown {
  return [{ type: "text", text: JSON.stringify(obj) }];
}

describe("IdeationWidget", () => {
  describe("PlanCreated (create_plan_artifact)", () => {
    it("extracts name from MCP-wrapped result", () => {
      const toolCall = makeToolCall("create_plan_artifact", {
        result: mcpWrap({ name: "My New Plan", version: 1 }),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("My New Plan")).toBeInTheDocument();
      expect(screen.getByText("Plan created")).toBeInTheDocument();
    });

    it("falls back to arguments.title when result has no name", () => {
      const toolCall = makeToolCall("create_plan_artifact", {
        arguments: { title: "From Args" },
        result: mcpWrap({ version: 1 }),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("From Args")).toBeInTheDocument();
    });

    it("falls back to 'Plan' when result and args both missing name", () => {
      const toolCall = makeToolCall("create_plan_artifact", {
        result: mcpWrap({}),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("Plan")).toBeInTheDocument();
    });

    it("extracts name from plain object result (passthrough)", () => {
      const toolCall = makeToolCall("create_plan_artifact", {
        result: { name: "Plain Object Plan" },
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("Plain Object Plan")).toBeInTheDocument();
    });
  });

  describe("AskUserQuestion (ask_user_question)", () => {
    it("uses stable non-compact row geometry for snapshots", () => {
      const toolCall = makeToolCall("ask_user_question", {
        arguments: { header: "Preferred default for automatic PR creation?" },
      });

      render(<IdeationWidget toolCall={toolCall} />);

      const widget = screen.getByTestId("ideation-widget-ask-question");
      expect(widget).toHaveStyle({ display: "flow-root" });
      expect(widget.firstElementChild).toHaveStyle({
        display: "flex",
        height: "26px",
        boxSizing: "border-box",
      });
      expect(screen.getByText("Preferred default for automatic PR creation?")).toBeInTheDocument();
      expect(screen.getByText("Question")).toBeInTheDocument();
    });
  });

  describe("GetProposal (get_proposal)", () => {
    it("extracts title and category from MCP-wrapped result", () => {
      const toolCall = makeToolCall("get_proposal", {
        result: mcpWrap({ title: "Add Auth", category: "feature" }),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("Add Auth")).toBeInTheDocument();
      expect(screen.getByText("feature")).toBeInTheDocument();
    });

    it("shows 'Loading proposal...' when result has no title", () => {
      const toolCall = makeToolCall("get_proposal", {
        result: mcpWrap({}),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("Loading proposal...")).toBeInTheDocument();
    });

    it("extracts title from plain object result", () => {
      const toolCall = makeToolCall("get_proposal", {
        result: { title: "Fix Bug", category: "bug" },
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("Fix Bug")).toBeInTheDocument();
    });
  });

  describe("GetSessionPlan (get_session_plan)", () => {
    it("extracts name and version from MCP-wrapped result", () => {
      const toolCall = makeToolCall("get_session_plan", {
        result: mcpWrap({ name: "Session Plan Alpha", version: 3 }),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("Session Plan Alpha")).toBeInTheDocument();
      expect(screen.getByText("v3")).toBeInTheDocument();
    });

    it("shows 'No plan artifact' when result has no name", () => {
      const toolCall = makeToolCall("get_session_plan", {
        result: mcpWrap({}),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("No plan artifact")).toBeInTheDocument();
    });

    it("extracts name from plain object result", () => {
      const toolCall = makeToolCall("get_session_plan", {
        result: { name: "Plain Plan", version: 2 },
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("Plain Plan")).toBeInTheDocument();
      expect(screen.getByText("v2")).toBeInTheDocument();
    });
  });

  describe("FinalizeProposals (finalize_proposals)", () => {
    it("shows task count from MCP-wrapped result", () => {
      const toolCall = makeToolCall("mcp__ralphx__finalize_proposals", {
        result: mcpWrap({
          created_task_ids: ["t1", "t2", "t3"],
          session_status: "accepted",
        }),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("3 tasks created")).toBeInTheDocument();
    });

    it("shows singular 'task' when exactly one task created", () => {
      const toolCall = makeToolCall("mcp__ralphx__finalize_proposals", {
        result: mcpWrap({
          created_task_ids: ["t1"],
          session_status: "accepted",
        }),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("1 task created")).toBeInTheDocument();
    });

    it("shows 'No tasks created' when created_task_ids is empty", () => {
      const toolCall = makeToolCall("mcp__ralphx__finalize_proposals", {
        result: mcpWrap({ created_task_ids: [], session_status: "active" }),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("No tasks created")).toBeInTheDocument();
    });

    it("renders Accepted status badge with success variant", () => {
      const toolCall = makeToolCall("mcp__ralphx__finalize_proposals", {
        result: mcpWrap({
          created_task_ids: ["t1"],
          session_status: "accepted",
        }),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("Accepted")).toBeInTheDocument();
    });

    it("renders non-accepted status badge", () => {
      const toolCall = makeToolCall("mcp__ralphx__finalize_proposals", {
        result: mcpWrap({
          created_task_ids: [],
          session_status: "active",
        }),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("Active")).toBeInTheDocument();
    });

    it("renders deps badge when dependencies_created > 0", () => {
      const toolCall = makeToolCall("mcp__ralphx__finalize_proposals", {
        result: mcpWrap({
          created_task_ids: ["t1"],
          dependencies_created: 4,
          session_status: "accepted",
        }),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("4 deps")).toBeInTheDocument();
    });

    it("renders warnings badge when warnings are present", () => {
      const toolCall = makeToolCall("mcp__ralphx__finalize_proposals", {
        result: mcpWrap({
          created_task_ids: ["t1"],
          session_status: "accepted",
          warnings: ["warn1", "warn2"],
        }),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("2 warnings")).toBeInTheDocument();
    });

    it("shows loading fallback when result is missing", () => {
      const toolCall = makeToolCall("mcp__ralphx__finalize_proposals", {
        result: undefined,
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("Finalizing proposals...")).toBeInTheDocument();
    });
  });

  describe("CrossProjectGuide (cross_project_guide)", () => {
    it("shows path count when cross-project paths detected", () => {
      const toolCall = makeToolCall("mcp__ralphx__cross_project_guide", {
        result: mcpWrap({
          has_cross_project_paths: true,
          detected_paths: ["proj-a", "proj-b"],
          gate_status: "set",
        }),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("2 projects detected")).toBeInTheDocument();
      expect(screen.getByText("Cross-project")).toBeInTheDocument();
    });

    it("shows singular 'project' when one path detected", () => {
      const toolCall = makeToolCall("mcp__ralphx__cross_project_guide", {
        result: mcpWrap({
          has_cross_project_paths: true,
          detected_paths: ["proj-a"],
          gate_status: "set",
        }),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("1 project detected")).toBeInTheDocument();
    });

    it("shows 'No cross-project paths' when flag is false", () => {
      const toolCall = makeToolCall("mcp__ralphx__cross_project_guide", {
        result: mcpWrap({
          has_cross_project_paths: false,
          detected_paths: [],
          gate_status: "no_session_id",
        }),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("No cross-project paths")).toBeInTheDocument();
      expect(screen.getByText("Single project")).toBeInTheDocument();
    });

    it("renders 'Gate set' label for gate_status=set", () => {
      const toolCall = makeToolCall("mcp__ralphx__cross_project_guide", {
        result: mcpWrap({
          has_cross_project_paths: true,
          detected_paths: ["proj-a"],
          gate_status: "set",
        }),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("Gate set")).toBeInTheDocument();
    });

    it("renders 'No gate' label for gate_status=no_session_id", () => {
      const toolCall = makeToolCall("mcp__ralphx__cross_project_guide", {
        result: mcpWrap({
          has_cross_project_paths: false,
          detected_paths: [],
          gate_status: "no_session_id",
        }),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("No gate")).toBeInTheDocument();
    });

    it("renders 'Gate error' label for gate_status=backend_unavailable", () => {
      const toolCall = makeToolCall("mcp__ralphx__cross_project_guide", {
        result: mcpWrap({
          has_cross_project_paths: false,
          detected_paths: [],
          gate_status: "backend_unavailable",
        }),
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("Gate error")).toBeInTheDocument();
    });

    it("shows loading fallback when result is missing", () => {
      const toolCall = makeToolCall("mcp__ralphx__cross_project_guide", {
        result: undefined,
      });
      render(<IdeationWidget toolCall={toolCall} />);
      expect(screen.getByText("Analyzing cross-project paths...")).toBeInTheDocument();
    });
  });
});
