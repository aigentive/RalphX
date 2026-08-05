import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";
import { useUiStore } from "@/stores/uiStore";
import { RecoveryPromptDialog } from "./RecoveryPromptDialog";

const { resolveRecoveryPrompt, gateState, toastInfo } = vi.hoisted(() => ({
  resolveRecoveryPrompt: vi.fn(),
  gateState: { status: "enabled" },
  toastInfo: vi.fn(),
}));
vi.mock("@/api/recovery", () => ({ resolveRecoveryPrompt }));
vi.mock("@/hooks/useAgentGate", () => ({ useAgentGate: () => gateState }));
vi.mock("sonner", () => ({ toast: { info: toastInfo, error: vi.fn() } }));

const taskId = "11111111-1111-4111-8111-111111111111";
const prompt = {
  taskId,
  status: "stopped" as const,
  contextType: "execution" as const,
  reason: "Choose recovery",
  primaryAction: { id: "restart" as const, label: "Restart" },
  secondaryAction: { id: "cancel" as const, label: "Cancel" },
};

describe("RecoveryPromptDialog", () => {
  beforeEach(() => {
    resolveRecoveryPrompt.mockReset().mockResolvedValue(true);
    toastInfo.mockReset();
    gateState.status = "enabled";
    useEnvironmentStore.setState({
      activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
      environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
    });
    useUiStore.setState({ recoveryPrompt: prompt, recoveryPromptSurface: "task_detail" });
  });

  it("keeps local recovery actionable", async () => {
    render(<RecoveryPromptDialog taskId={taskId} surface="task_detail" />);
    fireEvent.click(screen.getByRole("button", { name: "Restart" }));
    await waitFor(() => expect(resolveRecoveryPrompt).toHaveBeenCalledWith(taskId, "restart"));
    await waitFor(() => expect(useUiStore.getState().recoveryPrompt).toBeNull());
  });

  it("degrades an older remote host to notice and dismiss", () => {
    gateState.status = "unavailable";
    useEnvironmentStore.setState({
      activeEnvironmentId: "remote-1",
      environments: [{ id: "remote-1", name: "Studio", kind: "remote" }],
    });
    useUiStore.setState({ recoveryPrompt: prompt, recoveryPromptSurface: "task_detail" });
    render(<RecoveryPromptDialog taskId={taskId} surface="task_detail" />);
    expect(screen.getByTestId("remote-host-only-notice")).toHaveTextContent("Studio");
    expect(screen.queryByRole("button", { name: "Restart" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(resolveRecoveryPrompt).not.toHaveBeenCalled();
    expect(useUiStore.getState().recoveryPrompt).toBeNull();
  });

  it("clears benign applied:false with a neutral host-resolution message", async () => {
    useEnvironmentStore.setState({
      activeEnvironmentId: "remote-1",
      environments: [{ id: "remote-1", name: "Studio", kind: "remote" }],
    });
    // Wave E1 clears device-local dialog state on environment change; rehydrate B5d's host prompt.
    useUiStore.setState({ recoveryPrompt: prompt, recoveryPromptSurface: "task_detail" });
    resolveRecoveryPrompt.mockResolvedValue(false);
    render(<RecoveryPromptDialog taskId={taskId} surface="task_detail" />);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(useUiStore.getState().recoveryPrompt).toBeNull());
    expect(toastInfo).toHaveBeenCalledWith("Recovery was resolved on the host");
  });

  it("keeps the prompt when the real remote command-error envelope rejects", async () => {
    useEnvironmentStore.setState({
      activeEnvironmentId: "remote-1",
      environments: [{ id: "remote-1", name: "Studio", kind: "remote" }],
    });
    // Wave E1 clears device-local dialog state on environment change; rehydrate B5d's host prompt.
    useUiStore.setState({ recoveryPrompt: prompt, recoveryPromptSurface: "task_detail" });
    resolveRecoveryPrompt.mockRejectedValue({
      outcome: "commandError",
      error: "REMOTE_RESUME_AUTHORITY_CHANGED",
    });
    render(<RecoveryPromptDialog taskId={taskId} surface="task_detail" />);
    fireEvent.click(screen.getByRole("button", { name: "Restart" }));
    await waitFor(() => expect(resolveRecoveryPrompt).toHaveBeenCalled());
    expect(useUiStore.getState().recoveryPrompt).toEqual(prompt);
  });
});
