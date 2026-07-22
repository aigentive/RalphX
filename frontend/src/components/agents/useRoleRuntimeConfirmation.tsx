import { useCallback, useRef } from "react";

import { manualRoleDefaultsApi } from "@/api/manual-role-defaults";
import { harnessProvidersApi } from "@/api/harness-providers";
import type { ManualRoleRuntimeSelection } from "@/api/manual-role-defaults.types";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useConfirmation, type ConfirmOptions } from "@/hooks/useConfirmation";
import { usePersonas } from "@/hooks/usePersonas";
import { extractErrorMessage } from "@/lib/errors";
import {
  useAgentSessionStore,
  type LaunchRuntimeRoleKey,
} from "@/stores/agentSessionStore";

import { RoleRuntimeConfirmationBody } from "./RoleRuntimeConfirmationBody";
import { buildAgentProviderAvailabilityOptions } from "./agentProviderAvailability";
import { getManualRoleRuntimeSelectionIssue } from "./composer/runtime/manualRoleRuntimeValidation";

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
      onConfirm: (selection: ManualRoleRuntimeSelection) => Promise<unknown>;
    }) => {
      if (!conversationId) return Promise.resolve(false);
      latestSelectionRef.current = null;
      return confirm({
        title,
        description,
        confirmText,
        ...(pendingText ? { pendingText } : {}),
        confirmDisabled: true,
        prepare: async (controller) => {
          const [catalog, providerSettings, preparedDescription] = await Promise.all([
            manualRoleDefaultsApi.list(projectId),
            harnessProvidersApi.list({ refreshRuntime: true }),
            prepareDescription?.(),
          ]);
          if (!controller.isCurrent()) return {};
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
          return {
            confirmDisabled: Boolean(initialIssue),
            ...(preparedDescription ? { description: preparedDescription } : {}),
            body: (
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
            ),
          };
        },
        ...(recoverFromPrepareError ? { recoverFromPrepareError } : {}),
        onConfirm: async () => {
          const selection = latestSelectionRef.current;
          if (!selection) throw new Error("Runtime selection is not ready");
          await onConfirm({ ...selection });
        },
        recoverFromError: async (error) =>
          (await recoverFromError?.(error)) ?? {
            description: extractErrorMessage(
              error,
              "The action did not start. Review the runtime and try again.",
            ),
          },
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
