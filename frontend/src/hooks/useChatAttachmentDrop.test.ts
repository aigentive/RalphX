import { act, renderHook, waitFor } from "@testing-library/react";
import type { RefObject } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useChatAttachmentDrop } from "./useChatAttachmentDrop";

type DragPayload = {
  type: string;
  paths?: string[];
  position?: { x: number; y: number };
};

type DragHandler = (event: { payload: DragPayload }) => void | Promise<void>;

const mocks = vi.hoisted(() => ({
  dragDropHandler: null as DragHandler | null,
  onDragDropEvent: vi.fn(),
  readFile: vi.fn(),
  stat: vi.fn(),
  unlisten: vi.fn(),
}));

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (handler: DragHandler) => {
      mocks.dragDropHandler = handler;
      mocks.onDragDropEvent(handler);
      return Promise.resolve(() => {
        mocks.dragDropHandler = null;
        mocks.unlisten();
      });
    },
  }),
}));

vi.mock("@tauri-apps/plugin-fs", () => ({
  readFile: (path: string) => mocks.readFile(path),
  stat: (path: string) => mocks.stat(path),
}));

function makeTargetRef(): RefObject<HTMLElement | null> {
  const element = document.createElement("div");
  element.getBoundingClientRect = () =>
    ({
      left: 10,
      top: 20,
      right: 210,
      bottom: 120,
      width: 200,
      height: 100,
      x: 10,
      y: 20,
      toJSON: () => ({}),
    }) as DOMRect;

  return { current: element };
}

function makeHtmlDropEvent(files: File[]) {
  return {
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
    dataTransfer: {
      files,
      items: files.map((file) => ({
        kind: "file",
        type: file.type,
        getAsFile: () => file,
      })),
      types: ["Files"],
      dropEffect: "none",
    },
  } as unknown as React.DragEvent<HTMLElement>;
}

describe("useChatAttachmentDrop", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.dragDropHandler = null;
    mocks.stat.mockResolvedValue({
      size: 3,
      isFile: true,
      isDirectory: false,
      isSymlink: false,
    });
    mocks.readFile.mockResolvedValue(new Uint8Array([65, 66, 67]));
  });

  it("registers a native Tauri drop listener when enabled", async () => {
    renderHook(() =>
      useChatAttachmentDrop({
        enabled: true,
        targetRef: makeTargetRef(),
        onFilesSelected: vi.fn(),
      }),
    );

    await waitFor(() => expect(mocks.onDragDropEvent).toHaveBeenCalledTimes(1));
  });

  it("does not register a native Tauri listener when disabled", async () => {
    renderHook(() =>
      useChatAttachmentDrop({
        enabled: false,
        targetRef: makeTargetRef(),
        onFilesSelected: vi.fn(),
      }),
    );

    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(mocks.onDragDropEvent).not.toHaveBeenCalled();
  });

  it("shows drag state only when the native pointer is inside the target", async () => {
    const { result } = renderHook(() =>
      useChatAttachmentDrop({
        enabled: true,
        targetRef: makeTargetRef(),
        onFilesSelected: vi.fn(),
      }),
    );
    await waitFor(() => expect(mocks.dragDropHandler).not.toBeNull());

    act(() => {
      void mocks.dragDropHandler?.({ payload: { type: "over", position: { x: 30, y: 40 } } });
    });
    expect(result.current.isDragging).toBe(true);

    act(() => {
      void mocks.dragDropHandler?.({ payload: { type: "over", position: { x: 400, y: 40 } } });
    });
    expect(result.current.isDragging).toBe(false);
  });

  it("reads native dropped paths into File objects scoped to the target", async () => {
    const onFilesSelected = vi.fn();
    renderHook(() =>
      useChatAttachmentDrop({
        enabled: true,
        targetRef: makeTargetRef(),
        onFilesSelected,
      }),
    );
    await waitFor(() => expect(mocks.dragDropHandler).not.toBeNull());

    await act(async () => {
      await mocks.dragDropHandler?.({
        payload: {
          type: "drop",
          paths: ["/Users/dev/Desktop/note.txt"],
          position: { x: 30, y: 40 },
        },
      });
    });

    expect(mocks.stat).toHaveBeenCalledWith("/Users/dev/Desktop/note.txt");
    expect(mocks.readFile).toHaveBeenCalledWith("/Users/dev/Desktop/note.txt");
    expect(onFilesSelected).toHaveBeenCalledTimes(1);
    const droppedFiles = onFilesSelected.mock.calls[0]?.[0] as File[];
    expect(droppedFiles).toHaveLength(1);
    expect(droppedFiles[0]).toMatchObject({
      name: "note.txt",
      size: 3,
      type: "text/plain",
    });
  });

  it("ignores native drops outside the target", async () => {
    const onFilesSelected = vi.fn();
    renderHook(() =>
      useChatAttachmentDrop({
        enabled: true,
        targetRef: makeTargetRef(),
        onFilesSelected,
      }),
    );
    await waitFor(() => expect(mocks.dragDropHandler).not.toBeNull());

    await act(async () => {
      await mocks.dragDropHandler?.({
        payload: {
          type: "drop",
          paths: ["/Users/dev/Desktop/note.txt"],
          position: { x: 400, y: 40 },
        },
      });
    });

    expect(mocks.readFile).not.toHaveBeenCalled();
    expect(onFilesSelected).not.toHaveBeenCalled();
  });

  it("passes browser File drops through the same selection callback", () => {
    const onFilesSelected = vi.fn();
    const { result } = renderHook(() =>
      useChatAttachmentDrop({
        enabled: true,
        targetRef: makeTargetRef(),
        onFilesSelected,
      }),
    );
    const file = new File(["hello"], "browser.md", { type: "text/markdown" });
    const event = makeHtmlDropEvent([file]);

    act(() => {
      result.current.dropProps.onDrop(event);
    });

    expect(event.preventDefault).toHaveBeenCalled();
    expect(event.stopPropagation).toHaveBeenCalled();
    expect(onFilesSelected).toHaveBeenCalledWith([file]);
  });
});
