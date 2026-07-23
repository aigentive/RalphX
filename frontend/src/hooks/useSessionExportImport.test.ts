/**
 * useSessionExportImport hook tests
 *
 * Tests export and import of ideation sessions via Tauri file dialogs.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { createElement } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { useSessionExportImport } from "./useSessionExportImport";

// Mock dialog plugin
const mockOpen = vi.fn();
const mockSave = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => mockOpen(...args),
  save: (...args: unknown[]) => mockSave(...args),
}));

// Mock fs plugin
const mockReadTextFile = vi.fn();
const mockWriteTextFile = vi.fn();
const mockStat = vi.fn();
vi.mock("@tauri-apps/plugin-fs", () => ({
  readTextFile: (...args: unknown[]) => mockReadTextFile(...args),
  writeTextFile: (...args: unknown[]) => mockWriteTextFile(...args),
  stat: (...args: unknown[]) => mockStat(...args),
}));

// Mock sonner toast
const mockToastSuccess = vi.fn();
const mockToastError = vi.fn();
vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => mockToastSuccess(...args),
    error: (...args: unknown[]) => mockToastError(...args),
  },
}));

// Mock TanStack Query — only useQueryClient, keep QueryClient/QueryClientProvider real
const mockInvalidateQueries = vi.fn();
vi.mock("@tanstack/react-query", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@tanstack/react-query")>();
  return {
    ...actual,
    useQueryClient: () => ({ invalidateQueries: mockInvalidateQueries }),
  };
});

// Mock ideation store
const mockSetActiveSession = vi.fn();
vi.mock("@/stores/ideationStore", () => ({
  useIdeationStore: (selector: (state: { setActiveSession: typeof mockSetActiveSession }) => unknown) =>
    selector({ setActiveSession: mockSetActiveSession }),
}));

// ============================================================================
// Wrapper with QueryClientProvider (required by hook)
// ============================================================================

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

// ============================================================================
// Tests
// ============================================================================

describe("useSessionExportImport", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockWriteTextFile.mockResolvedValue(undefined);
    mockInvalidateQueries.mockResolvedValue(undefined);
  });

  describe("exportSession", () => {
    it("exports successfully and shows success toast", async () => {
      const jsonContent = '{"schema_version":2}';
      vi.mocked(invoke).mockResolvedValueOnce(jsonContent);
      mockSave.mockResolvedValueOnce("/tmp/test.ralphx-session");

      const { result } = renderHook(() => useSessionExportImport(), {
        wrapper: createWrapper(),
      });

      await act(async () => {
        await result.current.exportSession("session-123", "proj-456", true);
      });

      expect(invoke).toHaveBeenCalledWith("export_ideation_session", {
        id: "session-123",
        projectId: "proj-456",
      });
      expect(mockSave).toHaveBeenCalled();
      expect(mockWriteTextFile).toHaveBeenCalledWith(
        "/tmp/test.ralphx-session",
        jsonContent
      );
      expect(mockToastSuccess).toHaveBeenCalledWith("Session exported successfully");
    });

    it("does not write file when save dialog is cancelled", async () => {
      vi.mocked(invoke).mockResolvedValueOnce('{"schema_version":2}');
      mockSave.mockResolvedValueOnce(null);

      const { result } = renderHook(() => useSessionExportImport(), {
        wrapper: createWrapper(),
      });

      await act(async () => {
        await result.current.exportSession("session-123", "proj-456", true);
      });

      expect(mockWriteTextFile).not.toHaveBeenCalled();
      expect(mockToastSuccess).not.toHaveBeenCalled();
    });
  });

  describe("importSession", () => {
    it("imports successfully, activates session, and invalidates queries", async () => {
      mockOpen.mockResolvedValueOnce("/tmp/import.ralphx-session");
      mockStat.mockResolvedValueOnce({ size: 1024 });
      mockReadTextFile.mockResolvedValueOnce('{"key":"value"}');
      vi.mocked(invoke).mockResolvedValueOnce({
        sessionId: "new-123",
        title: "My Session",
        proposalCount: 5,
        planVersionCount: 2,
      });

      const { result } = renderHook(() => useSessionExportImport(), {
        wrapper: createWrapper(),
      });

      await act(async () => {
        await result.current.importSession("proj-456");
      });

      expect(mockReadTextFile).toHaveBeenCalledWith("/tmp/import.ralphx-session");
      expect(invoke).toHaveBeenCalledWith("import_ideation_session", {
        input: { jsonContent: '{"key":"value"}', projectId: "proj-456" },
      });
      expect(mockSetActiveSession).toHaveBeenCalledWith("new-123");
      expect(mockToastSuccess).toHaveBeenCalledWith(
        'Imported "My Session" (5 proposals)'
      );
      expect(mockInvalidateQueries).toHaveBeenCalled();
    });

    it("does nothing when open dialog is cancelled", async () => {
      mockOpen.mockResolvedValueOnce(null);

      const { result } = renderHook(() => useSessionExportImport(), {
        wrapper: createWrapper(),
      });

      await act(async () => {
        await result.current.importSession("proj-456");
      });

      expect(mockReadTextFile).not.toHaveBeenCalled();
      expect(invoke).not.toHaveBeenCalled();
      expect(mockToastError).not.toHaveBeenCalled();
    });

    it("shows error toast when file exceeds 10MB limit", async () => {
      mockOpen.mockResolvedValueOnce("/tmp/big.ralphx-session");
      mockStat.mockResolvedValueOnce({ size: 11_000_000 });

      const { result } = renderHook(() => useSessionExportImport(), {
        wrapper: createWrapper(),
      });

      await act(async () => {
        await result.current.importSession("proj-456");
      });

      expect(mockToastError).toHaveBeenCalledWith("File too large (max 10MB)");
      expect(mockReadTextFile).not.toHaveBeenCalled();
    });

    it("shows version unsupported message on IMPORT_VERSION_UNSUPPORTED error", async () => {
      mockOpen.mockResolvedValueOnce("/tmp/import.ralphx-session");
      mockStat.mockResolvedValueOnce({ size: 512 });
      mockReadTextFile.mockResolvedValueOnce('{"schema_version":1}');
      vi.mocked(invoke).mockRejectedValueOnce(
        new Error("IMPORT_VERSION_UNSUPPORTED: Schema version 1 is not supported")
      );

      const { result } = renderHook(() => useSessionExportImport(), {
        wrapper: createWrapper(),
      });

      await act(async () => {
        await result.current.importSession("proj-456");
      });

      expect(mockToastError).toHaveBeenCalledWith(
        "This file was created by a newer version of RalphX"
      );
    });
  });
});
