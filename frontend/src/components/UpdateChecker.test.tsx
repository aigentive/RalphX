import React from "react";
import { render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UpdateChecker } from "./UpdateChecker";

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  toast: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: (...args: unknown[]) => mocks.check(...args),
}));

vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: Object.assign(mocks.toast, {
    dismiss: vi.fn(),
    error: vi.fn(),
    loading: vi.fn(),
    success: vi.fn(),
  }),
}));

const update = {
  version: "0.3.2",
  currentVersion: "0.3.1",
  body: "Daily release",
  downloadAndInstall: vi.fn(),
};

describe("UpdateChecker", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mocks.check.mockReset();
    mocks.toast.mockReset();
    mocks.check.mockResolvedValue(update);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("checks after startup in React StrictMode", async () => {
    render(
      <React.StrictMode>
        <UpdateChecker />
      </React.StrictMode>,
    );

    expect(mocks.check).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(3_000);

    expect(mocks.check).toHaveBeenCalledTimes(1);
    expect(mocks.toast).toHaveBeenCalledTimes(1);
  });

  it("polls for later releases without re-notifying the same version", async () => {
    render(<UpdateChecker />);

    await vi.advanceTimersByTimeAsync(3_000);
    await vi.advanceTimersByTimeAsync(30 * 60 * 1_000);

    expect(mocks.check).toHaveBeenCalledTimes(2);
    expect(mocks.toast).toHaveBeenCalledTimes(1);
  });

  it("swallows check() errors silently", async () => {
    mocks.check.mockReset();
    mocks.check.mockRejectedValue(new Error("network down"));
    render(<UpdateChecker />);
    await vi.advanceTimersByTimeAsync(3_000);
    expect(mocks.toast).not.toHaveBeenCalled();
  });

  it("Later button dismisses the update toast", async () => {
    const toastModule = await import("sonner");
    render(<UpdateChecker />);
    await vi.advanceTimersByTimeAsync(3_000);

    const [body] = mocks.toast.mock.calls[0]!;
    interface Node {
      type?: unknown;
      props?: { children?: unknown[]; onClick?: () => void };
    }
    const root = body as Node;
    const buttonsRow = (root.props?.children as Node[] | undefined)?.[3] as Node | undefined;
    const buttons = (buttonsRow?.props?.children as Node[] | undefined) ?? [];
    const laterBtn = buttons[1];
    laterBtn!.props!.onClick!();
    expect(toastModule.toast.dismiss).toHaveBeenCalledWith("update-available");
  });

  it("Update Now triggers downloadAndInstall, progress events, success toast and relaunch", async () => {
    const downloadAndInstall = vi.fn().mockImplementation(async (cb) => {
      cb({ event: "Started", data: { contentLength: 100 } });
      cb({ event: "Progress", data: { chunkLength: 50 } });
      cb({ event: "Finished" });
    });
    const versioned = { ...update, version: "0.7.1", downloadAndInstall };
    mocks.check.mockReset();
    mocks.check.mockResolvedValue(versioned);

    const processMod = await import("@tauri-apps/plugin-process");
    const toastModule = await import("sonner");

    render(<UpdateChecker />);
    await vi.advanceTimersByTimeAsync(3_000);

    const [body] = mocks.toast.mock.calls[0]!;
    interface Node {
      type?: unknown;
      props?: { children?: unknown[]; onClick?: () => void };
    }
    const root = body as Node;
    const buttonsRow = (root.props?.children as Node[] | undefined)?.[3] as Node | undefined;
    const installBtn = ((buttonsRow?.props?.children as Node[] | undefined) ?? [])[0];
    // Drive the install handler.
    await installBtn!.props!.onClick!();

    expect(downloadAndInstall).toHaveBeenCalled();
    expect(toastModule.toast.success).toHaveBeenCalledWith(
      expect.stringContaining("Update installed"),
      expect.objectContaining({ id: "update-progress" }),
    );

    // Relaunch fires after the 1500ms grace.
    await vi.advanceTimersByTimeAsync(2_000);
    expect(processMod.relaunch).toHaveBeenCalled();
  });

  it("install failure shows an error toast and does not relaunch", async () => {
    const downloadAndInstall = vi.fn().mockRejectedValue(new Error("disk full"));
    const versioned = { ...update, version: "0.7.2", downloadAndInstall };
    mocks.check.mockReset();
    mocks.check.mockResolvedValue(versioned);

    const processMod = await import("@tauri-apps/plugin-process");
    const toastModule = await import("sonner");

    render(<UpdateChecker />);
    await vi.advanceTimersByTimeAsync(3_000);

    const [body] = mocks.toast.mock.calls[0]!;
    interface Node {
      type?: unknown;
      props?: { children?: unknown[]; onClick?: () => void };
    }
    const root = body as Node;
    const buttonsRow = (root.props?.children as Node[] | undefined)?.[3] as Node | undefined;
    const installBtn = ((buttonsRow?.props?.children as Node[] | undefined) ?? [])[0];
    await installBtn!.props!.onClick!();

    expect(toastModule.toast.error).toHaveBeenCalledWith(
      expect.stringContaining("Failed to install update"),
      expect.objectContaining({ id: "update-progress" }),
    );
    await vi.advanceTimersByTimeAsync(2_000);
    expect(processMod.relaunch).not.toHaveBeenCalled();
  });
});
