import { Repeat } from "lucide-react";
import { useCallback, useRef } from "react";

import { manualRoleDefaultsApi } from "@/api/manual-role-defaults";
import {
  harnessProvidersApi,
  type AgentProvidersSettingsResponse,
} from "@/api/harness-providers";
import type { ManualRoleRuntimeSelection } from "@/api/manual-role-defaults.types";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useConfirmation, type ConfirmOptions } from "@/hooks/useConfirmation";
import { harnessProviderKeys } from "@/hooks/useHarnessProviders";
import { usePersonas } from "@/hooks/usePersonas";
import { extractErrorMessage } from "@/lib/errors";
import { logger } from "@/lib/logger";
import { useQueryClient } from "@tanstack/react-query";
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
  const { confirm, confirmationDialogProps, ConfirmationDialog } =
    useConfirmation();
  const queryClient = useQueryClient();
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
      closeOnConfirm,
      onIntent,
      onErrorAfterClose,
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
      closeOnConfirm?: boolean;
      onIntent?: () => void;
      onErrorAfterClose?: (error: unknown) => void;
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
      let preparedDescriptionPromise: Promise<string | undefined> | null = null;
      let prepareDescriptionFailed = false;
      return confirm({
        title,
        description,
        confirmText,
        ...(pendingText ? { pendingText } : {}),
        confirmDisabled: true,
        prepare: async (controller) => {
          const prepareStartedAt = timingNow();
          preparedDescriptionPromise = runRoleRuntimeTimedPhase(
            role,
            "prepare_description",
            totalStartedAt,
            () => prepareDescription?.(),
          );
          const cachedProviderSettings =
            queryClient.getQueryData<AgentProvidersSettingsResponse>(
              harnessProviderKeys.list(false),
            );
          const [catalog, providerSettings] = await Promise.all([
            runRoleRuntimeTimedPhase(
              role,
              "load_role_defaults",
              totalStartedAt,
              () =>
                manualRoleDefaultsApi.list(projectId),
            ),
            runRoleRuntimeTimedPhase(
              role,
              cachedProviderSettings
                ? "load_cached_provider_runtime"
                : "refresh_provider_runtime",
              totalStartedAt,
              () =>
                cachedProviderSettings ??
                queryClient.fetchQuery({
                  queryKey: harnessProviderKeys.list(true),
                  queryFn: () =>
                    harnessProvidersApi.list({ refreshRuntime: true }),
                  staleTime: 0,
                }),
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
          const entry = catalog.roles.find((candidate) => candidate.role === role);
          if (!entry?.effective) {
            throw new Error(`No effective runtime is available for ${role}`);
          }
          const effective = entry.effective;
          const store = useAgentSessionStore.getState();
          const saved = store.roleRuntimeOverridesByConversationId[conversationId]?.[role];
          const buildPrepared = (
            settings: AgentProvidersSettingsResponse,
          ) => {
            const buildStartedAt = timingNow();
            const initial = latestSelectionRef.current ?? saved ?? effective;
            const providerOptions = buildAgentProviderAvailabilityOptions({
              providers: settings.providers,
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
              confirmDisabled: Boolean(initialIssue) || prepareDescriptionFailed,
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
                    controller.update({
                      confirmDisabled: Boolean(issue) || prepareDescriptionFailed,
                    });
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
            return prepared;
          };
          const prepared = buildPrepared(providerSettings);
          controller.update(prepared);
          void preparedDescriptionPromise
            ?.then((preparedDescription) => {
              prepareDescriptionFailed = false;
              if (preparedDescription) {
                controller.update({ description: preparedDescription });
              }
            })
            .catch((error: unknown) => {
              prepareDescriptionFailed = true;
              void Promise.resolve(recoverFromPrepareError?.(error) ?? null)
                .catch(() => null)
                .then((recovery) => {
                  if (recovery) {
                    controller.update(recovery);
                    return;
                  }
                  controller.update({
                    confirmDisabled: true,
                    description: "Could not prepare this action. Cancel and try again.",
                  });
                });
            });
          if (cachedProviderSettings) {
            void runRoleRuntimeTimedPhase(
              role,
              "refresh_provider_runtime",
              totalStartedAt,
              () =>
                queryClient.fetchQuery({
                  queryKey: harnessProviderKeys.list(true),
                  queryFn: () => harnessProvidersApi.list({ refreshRuntime: true }),
                  staleTime: 0,
                }),
            )
              .then((refreshedProviderSettings) => {
                if (!controller.isCurrent()) return;
                queryClient.setQueriesData<AgentProvidersSettingsResponse>(
                  { queryKey: harnessProviderKeys.all },
                  refreshedProviderSettings,
                );
                controller.update(buildPrepared(refreshedProviderSettings));
              })
              .catch(() => undefined);
          }
          logRoleRuntimeTiming(
            role,
            "prepare_completed",
            prepareStartedAt,
            totalStartedAt,
            "completed",
          );
          return {};
        },
        ...(recoverFromPrepareError ? { recoverFromPrepareError } : {}),
        ...(closeOnConfirm ? { closeOnConfirm } : {}),
        ...(onIntent ? { onIntent } : {}),
        ...(onErrorAfterClose ? { onErrorAfterClose } : {}),
        onConfirm: async () => {
          await preparedDescriptionPromise;
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
    [confirm, conversationId, personas, projectId, queryClient, registry],
  );

  return {
    confirmRoleRuntime,
    confirmationDialogProps,
    ConfirmationDialog,
  };
}
