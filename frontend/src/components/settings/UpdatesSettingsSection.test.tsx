import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { UpdatesSettingsSection } from "./UpdatesSettingsSection";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";

if (!HTMLElement.prototype.hasPointerCapture) {
  Object.defineProperty(HTMLElement.prototype, "hasPointerCapture", {
    value: () => false,
    writable: true,
  });
}

if (!HTMLElement.prototype.setPointerCapture) {
  Object.defineProperty(HTMLElement.prototype, "setPointerCapture", {
    value: vi.fn(),
    writable: true,
  });
}

if (!HTMLElement.prototype.releasePointerCapture) {
  Object.defineProperty(HTMLElement.prototype, "releasePointerCapture", {
    value: vi.fn(),
    writable: true,
  });
}

if (!HTMLElement.prototype.scrollIntoView) {
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    value: vi.fn(),
    writable: true,
  });
}

const mocks = vi.hoisted(() => ({
  updateChannel: "stable" as "stable" | "nightly",
  isLoading: false,
  loadError: null as Error | null,
  saveError: null as Error | null,
  isSaving: false,
  setUpdateChannel: vi.fn(),
}));

vi.mock("@/hooks/useUpdateChannel", () => ({
  useUpdateChannel: () => ({
    updateChannel: mocks.updateChannel,
    isLoading: mocks.isLoading,
    loadError: mocks.loadError,
    saveError: mocks.saveError,
    isSaving: mocks.isSaving,
    setUpdateChannel: mocks.setUpdateChannel,
  }),
}));

describe("UpdatesSettingsSection", () => {
  beforeEach(() => {
    mocks.updateChannel = "stable";
    mocks.isLoading = false;
    mocks.loadError = null;
    mocks.saveError = null;
    mocks.isSaving = false;
    mocks.setUpdateChannel.mockReset();
    useEnvironmentStore.setState({
      activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
      environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
    });
  });

  it("defaults to the selected Stable radio and saves an accessible Nightly selection", async () => {
    const user = userEvent.setup();
    render(<UpdatesSettingsSection />);

    expect(screen.getByRole("radiogroup", { name: "Update channel" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Stable — Recommended" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(screen.getByRole("radio", { name: "Nightly — Early access" })).toHaveAttribute(
      "aria-checked",
      "false",
    );

    await user.click(screen.getByRole("radio", { name: "Nightly — Early access" }));

    expect(mocks.setUpdateChannel).toHaveBeenCalledWith("nightly");
  });

  it("keeps both channel radios disabled while loading or saving", () => {
    const { rerender } = render(<UpdatesSettingsSection />);

    mocks.isLoading = true;
    rerender(<UpdatesSettingsSection />);
    expect(screen.getByRole("radio", { name: "Stable — Recommended" })).toBeDisabled();
    expect(screen.getByRole("radio", { name: "Nightly — Early access" })).toBeDisabled();

    mocks.isLoading = false;
    mocks.isSaving = true;
    rerender(<UpdatesSettingsSection />);
    expect(screen.getByRole("radio", { name: "Stable — Recommended" })).toBeDisabled();
    expect(screen.getByRole("radio", { name: "Nightly — Early access" })).toBeDisabled();
  });

  it("renders host-only disabled channel controls remotely without saving", async () => {
    useEnvironmentStore.setState({
      activeEnvironmentId: "remote-1",
      environments: [{ id: "remote-1", name: "Studio", kind: "remote" }],
    });
    render(<UpdatesSettingsSection />);
    const nightly = screen.getByRole("radio", { name: "Nightly — Early access" });
    expect(nightly).toBeDisabled();
    expect(screen.getByTestId("remote-host-only-notice")).toHaveTextContent("Studio");
    await userEvent.click(nightly);
    expect(mocks.setUpdateChannel).not.toHaveBeenCalled();
  });

  it("explains the Nightly safety boundary without exposing custom sources", () => {
    mocks.updateChannel = "nightly";
    render(<UpdatesSettingsSection />);

    expect(screen.getByRole("radio", { name: "Nightly — Early access" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(screen.getByText(
      /switching back to Stable stops future Nightly delivery but never downgrades the installed app/i,
    )).toBeInTheDocument();
    expect(screen.queryByText(/custom URL|branch|tag/i)).not.toBeInTheDocument();
  });

  it("keeps Stable selected and reports load and save persistence failures", () => {
    mocks.loadError = new Error("load failed");
    mocks.saveError = new Error("save failed");
    render(<UpdatesSettingsSection />);

    expect(screen.getByRole("radio", { name: "Stable — Recommended" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(screen.getByText(/unable to load update channel/i)).toBeInTheDocument();
    expect(screen.getByText(/unable to save update channel/i)).toBeInTheDocument();
  });
});
