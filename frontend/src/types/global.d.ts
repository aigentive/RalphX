/**
 * Global type declarations for window extensions
 * Used for Playwright testing in web mode
 */

import type { QueryClient } from "@tanstack/react-query";
import type { EventBus } from "@/lib/event-bus";
import type { MockStore } from "@/api-mock/store";
import type { MockChatController } from "@/api-mock/chat";
import type { GitAuthDiagnostics } from "@/hooks/useGithubSettings";

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
    // Playwright testing utilities (web mode only)
    __mockStore?: MockStore;
    __mockChatApi?: MockChatController;
    __mockGitAuthDiagnostics?: GitAuthDiagnostics;
    __mockGhAuthStatus?: boolean;
    __queryClient?: QueryClient;
    __eventBus?: EventBus;
    __uiStore?: unknown;
    __planStore?: {
      getState(): {
        loadActivePlan(projectId: string): Promise<void>;
        activePlanByProject: Record<string, string | null>;
        activeExecutionPlanIdByProject: Record<string, string | null>;
      };
    };
    __chatStore?: {
      getState(): {
        setActiveConversation(storeKey: string, conversationId: string | null): void;
        activeConversationIds?: Record<string, string | null | undefined>;
      };
    };
    __proposalStore?: {
      getState(): {
        setProposals(proposals: Record<string, unknown>[]): void;
      };
    };
    __ideationStore?: {
      getState(): {
        selectSession(session: Record<string, unknown>): void;
      };
    };
    __openReviewDetailModal?: (taskId: string) => void;
  }
}

export {};
