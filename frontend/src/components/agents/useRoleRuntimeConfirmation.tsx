import { Repeat } from "lucide-react";
import { useCallback, useRef } from "react";

import { manualRoleDefaultsApi } from "@/api/manual-role-defaults";
import { harnessProvidersApi } from "@/api/harness-providers";
import type { ManualRoleRuntimeSelection } from "@/api/manual-role-defaults.types";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useConfirmation, type ConfirmOptions } from "@/hooks/useConfirmation";
import { usePersonas } from "@/hooks/usePersonas";
import { extractErrorMessage } from "@/lib/errors";
import { logger } from "@/lib/logger";
import { Switch } from "@/components/ui/switch";
import {
  useAgentSessionStore,
  type LaunchRuntimeRoleKey,
} from "@/stores/agentSessionStore";

import { RoleRuntimeConfirmationBody } from "./RoleRuntimeConfirmationBody";
import { buildAgentProviderAvailabilityOptions } from "./agentProviderAvailability";
import { getManualRoleRuntimeSelectionIssue } from "./composer/runtime/manualRoleRuntimeValidation";

type RoleRuntimeTimingOutcome = "completed" | "failed" | "superseded";

function timingNow(): number {
  return globalThis.performance?.now() ?? Date.now();
}

function logRoleRuntimeTiming(
  role: LaunchRuntimeRoleKey,
  phase: string,
  phaseStartedAt: number,
  totalStartedAt: number,
  outcome: RoleRuntimeTimingOutcome,
): void {
  const now = timingNow();
  logger.debug("[RoleRuntimeConfirmationTiming]", {
    role,
    phase,
    elapsedMs: Math.max(0, Math.round(now - phaseStartedAt)),
    totalElapsedMs: Math.max(0, Math.round(now - totalStartedAt)),
    outcome,
  });
}

async function runRoleRuntimeTimedPhase<T>(
  role: LaunchRuntimeRoleKey,
  phase: string,
  totalStartedAt: number,
  work: () => Promise<T> | T,
): Promise<T> {
  const phaseStartedAt = timingNow();
  try {
    const result = await work();
    logRoleRuntimeTiming(
      role,
      phase,
      phaseStartedAt,
      totalStartedAt,
      "completed",
    );
    return result;
  } catch (error) {
    logRoleRuntimeTiming(
      role,
      phase,
      phaseStartedAt,
      totalStartedAt,
      "failed",
    );
    throw error;
  }
}

export function useRoleRuntimeConfirmation({
  conversationId,
  projectId,
}: {
  conversationId: string | null;
  projectId: string | null;
}) {
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const { registry } = useAgentModels();
  const { data: personas = [] } = usePersonas(
    projectId ? { type: "globalAndProject", projectId } : { type: "all" },
  );
  const latestSelectionRef = useRef<ManualRoleRuntimeSelection | null>(null);
  const latestOptInEnabledRef = useRef(false);

  const confirmRoleRuntime = useCallback(
    ({
      role,
      title,
      description,
      confirmText,
      pendingText,
      prepareDescription,
      recoverFromPrepareError,
      recoverFromError,
      optIn,
      onConfirm,
    }: {
      role: LaunchRuntimeRoleKey;
      title: string;
      description: string;
      confirmText: string;
      pendingText?: string;
      prepareDescription?: () => Promise<string>;
      recoverFromPrepareError?: (
        error: unknown,
      ) =>
        | Promise<
            Partial<
              Pick<
                ConfirmOptions,
                "description" | "confirmDisabled" | "bodyDisabled"
              >
            > | null
          >
        | Partial<
            Pick<
              ConfirmOptions,
              "description" | "confirmDisabled" | "bodyDisabled"
            >
          >
        | null;
      recoverFromError?: (
        error: unknown,
      ) =>
        | Promise<
            Partial<
              Pick<
                ConfirmOptions,
                "description" | "confirmDisabled" | "bodyDisabled"
              >
            > | null
          >
        | Partial<
            Pick<
              ConfirmOptions,
              "description" | "confirmDisabled" | "bodyDisabled"
            >
          >
        | null;
      optIn?: {
        title: string;
        description: string;
        initialValue: boolean;
        hidden?: boolean;
      };
      onConfirm: (
        selection: ManualRoleRuntimeSelection,
        optInEnabled?: boolean,
      ) => Promise<unknown>;
    }) => {
      if (!conversationId) return Promise.resolve(false);
      const totalStartedAt = timingNow();
      logRoleRuntimeTiming(
        role,
        "dialog_opened",
        totalStartedAt,
        totalStartedAt,
        "completed",
      );
      latestSelectionRef.current = null;
      latestOptInEnabledRef.current = optIn?.initialValue ?? false;
      return confirm({
        title,
        description,
        confirmText,
        ...(pendingText ? { pendingText } : {}),
        confirmDisabled: true,
        prepare: async (controller) => {
          const prepareStartedAt = timingNow();
          const [catalog, providerSettings, preparedDescription] = await Promise.all([
            runRoleRuntimeTimedPhase(
              role,
              "load_role_defaults",
              totalStartedAt,
              () => manualRoleDefaultsApi.list(projectId),
            ),
            runRoleRuntimeTimedPhase(
              role,
              "refresh_provider_runtime",
              totalStartedAt,
              () => harnessProvidersApi.list({ refreshRuntime: true }),
            ),
            runRoleRuntimeTimedPhase(
              role,
              "prepare_description",
              totalStartedAt,
              () => prepareDescription?.(),
            ),
          ]);
          if (!controller.isCurrent()) {
            logRoleRuntimeTiming(
              role,
              "prepare_completed",
              prepareStartedAt,
              totalStartedAt,
              "superseded",
            );
            return {};
          }
          const buildStartedAt = timingNow();
          const entry = catalog.roles.find((candidate) => candidate.role === role);
          if (!entry?.effective) {
            throw new Error(`No effective runtime is available for ${role}`);
          }
          const store = useAgentSessionStore.getState();
          const saved = store.roleRuntimeOverridesByConversationId[conversationId]?.[role];
          const initial: ManualRoleRuntimeSelection = saved ?? entry.effective;
          const providerOptions = buildAgentProviderAvailabilityOptions({
            providers: providerSettings.providers,
            isReady: true,
          });
          const initialIssue = getManualRoleRuntimeSelectionIssue({
            entry,
            value: initial,
            providerOptions,
            modelsForProvider: (provider) =>
              registry[provider as keyof typeof registry] ?? [],
            personas,
          });
          latestSelectionRef.current = initial;
          const prepared = {
            confirmDisabled: Boolean(initialIssue),
            ...(preparedDescription ? { description: preparedDescription } : {}),
            body: (
              <div className="space-y-3">
                <RoleRuntimeConfirmationBody
                  entry={entry}
                  initialValue={initial}
                  hasSavedOverride={Boolean(saved)}
                  modelRegistry={registry}
                  personas={personas}
                  providerOptions={providerOptions}
                  onChange={(selection) => {
                    latestSelectionRef.current = selection;
                    useAgentSessionStore
                      .getState()
                      .setRoleRuntimeOverride(conversationId, role, selection);
                  }}
                  onReset={(selection) => {
                    latestSelectionRef.current = selection;
                    useAgentSessionStore
                      .getState()
                      .clearRoleRuntimeOverride(conversationId, role);
                  }}
                  onValidityChange={(issue) => {
                    controller.update({ confirmDisabled: Boolean(issue) });
                  }}
                />
                {optIn && !optIn.hidden && (
                  <div
                    className="rounded-lg border p-3"
                    style={{
                      backgroundColor: "var(--bg-subtle)",
                      borderColor: "var(--border-subtle)",
                    }}
                  >
                    <div className="flex items-start gap-3">
                      <Repeat
                        className="mt-0.5 h-4 w-4 shrink-0 text-[var(--accent-primary)]"
                        aria-hidden="true"
                      />
                      <div className="min-w-0 flex-1">
                        <label className="flex min-h-8 items-center justify-between gap-3 text-sm font-medium text-[var(--text-primary)]">
                          <span>{optIn.title}</span>
                          <Switch
                            defaultChecked={optIn.initialValue}
                            onCheckedChange={(enabled) => {
                              latestOptInEnabledRef.current = enabled;
                            }}
                            aria-label={optIn.title}
                          />
                        </label>
                        <p className="mt-1 text-xs leading-relaxed text-[var(--text-secondary)]">
                          {optIn.description}
                        </p>
                      </div>
                    </div>
                  </div>
                )}
              </div>
            ),
          };
          logRoleRuntimeTiming(
            role,
            "build_confirmation",
            buildStartedAt,
            totalStartedAt,
            "completed",
          );
          logRoleRuntimeTiming(
            role,
            "prepare_completed",
            prepareStartedAt,
            totalStartedAt,
            "completed",
          );
          return prepared;
        },
        ...(recoverFromPrepareError ? { recoverFromPrepareError } : {}),
        onConfirm: async () => {
          const selection = latestSelectionRef.current;
          if (!selection) throw new Error("Runtime selection is not ready");
          await runRoleRuntimeTimedPhase(
            role,
            "confirm_action",
            totalStartedAt,
            () =>
              optIn
                ? onConfirm({ ...selection }, latestOptInEnabledRef.current)
                : onConfirm({ ...selection }),
          );
        },
        recoverFromError: async (error) =>
          runRoleRuntimeTimedPhase(
            role,
            "recover_confirm_error",
            totalStartedAt,
            async () =>
              (await recoverFromError?.(error)) ?? {
                description: extractErrorMessage(
                  error,
                  "The action did not start. Review the runtime and try again.",
                ),
              },
          ),
      });
    },
    [confirm, conversationId, personas, projectId, registry],
  );

  return {
    confirmRoleRuntime,
    confirmationDialogProps,
    ConfirmationDialog,
  };
}
