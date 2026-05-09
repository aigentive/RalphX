/**
 * useIdeationHandlers tests
 *
 * Tests in-flight deduplication guard for plan import handlers.
 */

import { describe, it, expect, vi, beforeEach, beforeAll } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useIdeationHandlers } from "./useIdeationHandlers";
import type { IdeationSession } from "@/types/ideation";

// Polyfill File.prototype.text for jsdom (not implemented in all jsdom versions)
beforeAll(() => {
  if (!File.prototype.text) {
    Object.defineProperty(File.prototype, "text", {
      value: function (this: Blob) {
        return Promise.resolve(new TextDecoder().decode(new Uint8Array()));
      },
      configurable: true,
    });
  }
});

// Mock ideationApi
vi.mock("@/api/ideation", () => ({
  ideationApi: {
    sessions: {
      spawnSessionNamer: vi.fn().mockResolvedValue(undefined),
    },
  },
}));

vi.mock("@/api/backend", () => ({
  backendApiUrl: (endpoint: string) => `http://backend.test/api/${endpoint}`,
}));

// Mock useIdeationStore — selector-based pattern used in the hook
const mockUpdateSession = vi.fn();
vi.mock("@/stores/ideationStore", () => ({
  useIdeationStore: (selector: (state: { updateSession: typeof mockUpdateSession }) => unknown) =>
    selector({ updateSession: mockUpdateSession }),
}));

const mockSession = {
  id: "session-123",
  title: "Test Session",
  projectId: "project-456",
} as unknown as IdeationSession;

type HandlerArgs = Parameters<typeof useIdeationHandlers>;

function buildProps(fetchPlanArtifact?: HandlerArgs[5]): HandlerArgs {
  return [
    mockSession,
    [],
    vi.fn(),
    vi.fn(),
    vi.fn(),
    fetchPlanArtifact ?? vi.fn().mockResolvedValue(undefined),
    vi.fn(),
    null,
  ];
}

function makeFileWithText(content = "# content", fileName = "plan.md"): File {
  const file = new File([content], fileName, { type: "text/markdown" });
  // Polyfill .text() per-instance to return actual content
  vi.spyOn(file, "text").mockResolvedValue(content);
  return file;
}

function createFileEvent(
  content = "# content",
  fileName = "plan.md"
): React.ChangeEvent<HTMLInputElement> {
  const file = makeFileWithText(content, fileName);
  return {
    target: { files: [file] as unknown as FileList, value: "" },
  } as unknown as React.ChangeEvent<HTMLInputElement>;
}

describe("useIdeationHandlers — in-flight guard", () => {
  let mockFetch: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.clearAllMocks();
    mockFetch = vi.fn();
    global.fetch = mockFetch;
  });

  // ---------------------------------------------------------------------------
  // handleFileDrop
  // ---------------------------------------------------------------------------

  describe("handleFileDrop guard", () => {
    it("imports through the configured backend API URL", async () => {
      const fetchPlanArtifact = vi.fn().mockResolvedValue(undefined);
      mockFetch.mockResolvedValueOnce(new Response(JSON.stringify({ id: "art-1" }), { status: 200 }));

      const { result } = renderHook(() => useIdeationHandlers(...buildProps(fetchPlanArtifact)));
      const file = makeFileWithText("# imported", "imported_plan.md");

      await act(async () => {
        await result.current.handleFileDrop(file, "# imported");
      });

      expect(mockFetch).toHaveBeenCalledWith(
        "http://backend.test/api/create_plan_artifact",
        expect.objectContaining({
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            session_id: "session-123",
            title: "imported plan",
            content: "# imported",
          }),
        })
      );
      expect(fetchPlanArtifact).toHaveBeenCalledWith("art-1");
    });

    it("blocks a concurrent call when already importing", async () => {
      let resolveFetch!: (value: Response) => void;
      mockFetch.mockReturnValueOnce(
        new Promise<Response>((resolve) => {
          resolveFetch = resolve;
        })
      );

      const { result } = renderHook(() => useIdeationHandlers(...buildProps()));
      const file = makeFileWithText();

      // First call starts (in-flight)
      act(() => {
        void result.current.handleFileDrop(file, "# content");
      });

      // Second call arrives before first resolves — must be blocked
      await act(async () => {
        await result.current.handleFileDrop(file, "# content");
      });

      expect(mockFetch).toHaveBeenCalledTimes(1);

      // Cleanup: resolve first fetch to avoid open handles
      resolveFetch(new Response(JSON.stringify({ id: "art-1" }), { status: 200 }));
    });

    it("resets guard after successful import so next import can proceed", async () => {
      mockFetch
        .mockResolvedValueOnce(new Response(JSON.stringify({ id: "art-1" }), { status: 200 }))
        .mockResolvedValueOnce(new Response(JSON.stringify({ id: "art-2" }), { status: 200 }));

      const { result } = renderHook(() => useIdeationHandlers(...buildProps()));
      const file = makeFileWithText();

      await act(async () => {
        await result.current.handleFileDrop(file, "# content");
      });
      await act(async () => {
        await result.current.handleFileDrop(file, "# content");
      });

      expect(mockFetch).toHaveBeenCalledTimes(2);
    });

    it("resets guard after error so future imports can proceed", async () => {
      mockFetch
        .mockResolvedValueOnce(new Response(null, { status: 500 }))
        .mockResolvedValueOnce(new Response(JSON.stringify({ id: "art-2" }), { status: 200 }));

      const { result } = renderHook(() => useIdeationHandlers(...buildProps()));
      const file = makeFileWithText();

      // First call fails
      await act(async () => {
        await result.current.handleFileDrop(file, "# content");
      });

      // Guard must be released — second call goes through
      await act(async () => {
        await result.current.handleFileDrop(file, "# content");
      });

      expect(mockFetch).toHaveBeenCalledTimes(2);
    });
  });

  // ---------------------------------------------------------------------------
  // handleFileSelected
  // ---------------------------------------------------------------------------

  describe("handleFileSelected guard", () => {
    it("blocks a concurrent call when already importing", async () => {
      let resolveFetch!: (value: Response) => void;
      mockFetch.mockReturnValueOnce(
        new Promise<Response>((resolve) => {
          resolveFetch = resolve;
        })
      );

      const { result } = renderHook(() => useIdeationHandlers(...buildProps()));

      // First call starts (in-flight)
      act(() => {
        void result.current.handleFileSelected(createFileEvent());
      });

      // Yield a microtask so file.text() resolves and fetch is entered before second call
      await act(async () => {
        await Promise.resolve();
      });

      // Second call blocked by guard (fetch still pending)
      await act(async () => {
        await result.current.handleFileSelected(createFileEvent());
      });

      expect(mockFetch).toHaveBeenCalledTimes(1);

      // Cleanup
      resolveFetch(new Response(JSON.stringify({ id: "art-1" }), { status: 200 }));
    });

    it("resets guard after error so future imports can proceed", async () => {
      mockFetch
        .mockResolvedValueOnce(new Response(null, { status: 500 }))
        .mockResolvedValueOnce(new Response(JSON.stringify({ id: "art-2" }), { status: 200 }));

      const { result } = renderHook(() => useIdeationHandlers(...buildProps()));

      await act(async () => {
        await result.current.handleFileSelected(createFileEvent());
      });
      await act(async () => {
        await result.current.handleFileSelected(createFileEvent());
      });

      expect(mockFetch).toHaveBeenCalledTimes(2);
    });
  });

  // ---------------------------------------------------------------------------
  // Cross-handler guard (shared isImportingRef)
  // ---------------------------------------------------------------------------

  describe("cross-handler guard", () => {
    it("blocks handleFileDrop while handleFileSelected is in-flight", async () => {
      let resolveFetch!: (value: Response) => void;
      mockFetch.mockReturnValueOnce(
        new Promise<Response>((resolve) => {
          resolveFetch = resolve;
        })
      );

      const { result } = renderHook(() => useIdeationHandlers(...buildProps()));
      const file = makeFileWithText();

      // handleFileSelected starts first
      act(() => {
        void result.current.handleFileSelected(createFileEvent());
      });

      // Yield microtask so file.text() resolves and fetch is entered
      await act(async () => {
        await Promise.resolve();
      });

      // handleFileDrop must be blocked by the same guard
      await act(async () => {
        await result.current.handleFileDrop(file, "# content");
      });

      expect(mockFetch).toHaveBeenCalledTimes(1);

      // Cleanup
      resolveFetch(new Response(JSON.stringify({ id: "art-1" }), { status: 200 }));
    });

    it("blocks handleFileSelected while handleFileDrop is in-flight", async () => {
      let resolveFetch!: (value: Response) => void;
      mockFetch.mockReturnValueOnce(
        new Promise<Response>((resolve) => {
          resolveFetch = resolve;
        })
      );

      const { result } = renderHook(() => useIdeationHandlers(...buildProps()));
      const file = makeFileWithText();

      // handleFileDrop starts first
      act(() => {
        void result.current.handleFileDrop(file, "# content");
      });

      // handleFileSelected must be blocked (handleFileDrop has no file.text() await, so guard is set synchronously)
      await act(async () => {
        await result.current.handleFileSelected(createFileEvent());
      });

      expect(mockFetch).toHaveBeenCalledTimes(1);

      // Cleanup
      resolveFetch(new Response(JSON.stringify({ id: "art-1" }), { status: 200 }));
    });
  });
});
