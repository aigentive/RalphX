// PR 2.5-b — the Connections settings pane.
//
// Every staged state shown here comes from `list_remote_environments` statuses and
// nothing else. The reconciler's report is in-memory with no durable carrier, so
// inferring "removing" or "finishing setup" from anything local would be inventing
// lifecycle the backend never asserted. A row leaves this list when Rust says it has,
// never optimistically.

import { useCallback, useEffect, useState } from "react";
import { Loader2, Trash2 } from "lucide-react";

import {
  remoteEnvironmentsApi,
  type RemoteEnvironmentSummary,
} from "@/api/remote-environments";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { CopyableRef } from "@/components/ui/copyable-ref";
import { NoticeBanner } from "@/components/ui/notice-banner";
import { StatusPill } from "@/components/ui/status-pill";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useClientOwnedFeatureFlag } from "@/lib/remote/feature-flag-authority";
import { clearEnvScopedStorage } from "@/lib/remote/env-scoped-storage";
import { useEnvironmentStore } from "@/stores/environmentStore";

import {
  RemoteAccessCardHeader,
  RemoteAccessSkeletonRows,
} from "../remote-access/RemoteAccessSection";
import { usePaintBoundaryHydration } from "../usePaintBoundaryHydration";
import { AddEnvironmentDialog } from "./AddEnvironmentDialog";

function errorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error) {
    return error.message;
  }
  return typeof error === "string" && error.length > 0 ? error : fallback;
}

interface RowPresentation {
  label: string;
  tone: "neutral" | "success" | "warning" | "accent";
  explanation: string | null;
}

/**
 * Status → presentation. `active` shows `Paired`, deliberately NOT a live connection
 * dot: this pane knows the registry, not the socket. Painting a green "Connected" from
 * a row status would assert liveness nothing here has observed.
 */
function presentRow(environment: RemoteEnvironmentSummary): RowPresentation {
  switch (environment.status) {
    case "active":
      return { label: "Paired", tone: "success", explanation: null };
    case "pending_delete":
      return {
        label: "Removing…",
        tone: "warning",
        explanation:
          "Removal in progress — host revoke and credential cleanup finish automatically (retried at next launch). No action needed.",
      };
    case "pending_add":
      return {
        label: "Finishing setup…",
        tone: "warning",
        explanation: "Pairing was interrupted.",
      };
  }
}

export function ConnectionsSection() {
  // Client-owned flag: whether THIS client runs remote environments is never a
  // host's answer, so read the local uiStore copy, not the env-scoped query.
  const enabled = useClientOwnedFeatureFlag("remoteEnvironments");
  // Inert while the flag is off (§8 flags note): no shell, no invokes, no listeners.
  if (!enabled) {
    return null;
  }
  return <ConnectionsPanel />;
}

function ConnectionsPanel() {
  const hydrated = usePaintBoundaryHydration();
  const [environments, setEnvironments] = useState<
    RemoteEnvironmentSummary[] | null
  >(null);
  // Two slots, not one. A failed list and a failed removal are different facts, and
  // folding them together let the re-list that FOLLOWS a failed removal clear the very
  // error it was reporting — the failure vanished before the user could read it.
  const [listError, setListError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [removing, setRemoving] = useState<RemoteEnvironmentSummary | null>(
    null,
  );
  const [busyRowId, setBusyRowId] = useState<string | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [rePairTarget, setRePairTarget] =
    useState<RemoteEnvironmentSummary | null>(null);

  const refresh = useCallback(async () => {
    try {
      const rows = await remoteEnvironmentsApi.list();
      setEnvironments(rows);
      setListError(null);
    } catch (caught) {
      // The previously loaded list is preserved: a failed refresh must not look like
      // "you have no environments", which is a different and much scarier claim.
      setListError(
        errorMessage(caught, "Could not read the environment registry."),
      );
    }
  }, []);

  useEffect(() => {
    if (!hydrated) {
      return;
    }
    void refresh();
  }, [hydrated, refresh]);

  const handleRemove = useCallback(
    async (environment: RemoteEnvironmentSummary) => {
      setBusyRowId(environment.id);
      setActionError(null);
      try {
        await remoteEnvironmentsApi.remove(environment.id);
        // P-27: the row's env-scoped UI state goes with it. Only after Rust accepted
        // the staged removal — clearing first would discard state for an environment
        // that might still be there.
        clearEnvScopedStorage(environment.id);
        await useEnvironmentStore.getState().loadEnvironments();
      } catch (caught) {
        setActionError(
          errorMessage(caught, `Could not remove “${environment.name}”.`),
        );
      } finally {
        setBusyRowId(null);
        // Always re-list: the staged machine may have advanced to pending_delete even
        // when the call surfaced an error, and the user must see the real state.
        await refresh();
      }
    },
    [refresh],
  );

  const openRePair = useCallback((environment: RemoteEnvironmentSummary) => {
    setRePairTarget(environment);
    setAddOpen(true);
  }, []);

  const openAdd = useCallback(() => {
    setRePairTarget(null);
    setAddOpen(true);
  }, []);

  return (
    <div className="space-y-4" data-testid="connections-section">
      <Card
        className="overflow-hidden"
        style={{
          backgroundColor: "var(--card-bg, var(--bg-elevated))",
          borderColor: "var(--border-subtle, #2c2c33)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        <RemoteAccessCardHeader
          title="Connections"
          description="Remote RalphX environments this Mac can connect to and operate."
        />
        <div className="space-y-3 p-5 pt-0">
          <div className="flex justify-end">
            <Button
              type="button"
              size="sm"
              data-testid="connections-add"
              onClick={openAdd}
            >
              Add environment
            </Button>
          </div>

          {(actionError ?? listError) !== null && (
            <NoticeBanner tone="error" testId="connections-error">
              {actionError ?? listError}
            </NoticeBanner>
          )}

          {environments === null ? (
            <RemoteAccessSkeletonRows rows={2} />
          ) : environments.length === 0 ? (
            <div
              className="rounded-md p-4 text-center"
              data-testid="connections-empty"
              style={{
                backgroundColor: "var(--bg-surface, #1e1e23)",
                borderColor: "var(--border-subtle, #2c2c33)",
                borderStyle: "dashed",
                borderWidth: "1px",
              }}
            >
              <p className="text-xs text-[var(--text-primary)]">
                No remote environments yet.
              </p>
              <p className="mt-0.5 text-xs text-[var(--text-muted)]">
                Pair this Mac with a host running Remote Access.
              </p>
              <Button
                type="button"
                size="sm"
                className="mt-3"
                data-testid="connections-empty-add"
                onClick={openAdd}
              >
                Add environment
              </Button>
            </div>
          ) : (
            <ul className="space-y-2" data-testid="connections-list">
              {environments.map((environment) => {
                const presentation = presentRow(environment);
                return (
                  <li
                    key={environment.id}
                    data-testid={`connections-row-${environment.id}`}
                    data-status={environment.status}
                    className="rounded-md p-3"
                    style={{
                      backgroundColor: "var(--bg-surface, #1e1e23)",
                      borderColor: "var(--border-subtle, #2c2c33)",
                      borderStyle: "solid",
                      borderWidth: "1px",
                    }}
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <p className="truncate text-xs font-medium text-[var(--text-primary)]">
                          {environment.name}
                        </p>
                        <CopyableRef
                          value={environment.baseUrl}
                          ariaLabel={`Copy ${environment.name} address`}
                          testId={`connections-url-${environment.id}`}
                        />
                      </div>
                      <StatusPill
                        tone={presentation.tone}
                        label={presentation.label}
                        testId={`connections-status-${environment.id}`}
                      />
                    </div>

                    {presentation.explanation !== null && (
                      <p
                        className="mt-2 text-xs text-[var(--text-muted)]"
                        data-testid={`connections-explanation-${environment.id}`}
                      >
                        {presentation.explanation}
                      </p>
                    )}

                    {/* pending_delete gets NO actions: the reconciler owns that row. */}
                    {environment.status !== "pending_delete" && (
                      <div className="mt-2 flex items-center justify-end gap-2">
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          data-testid={`connections-repair-${environment.id}`}
                          disabled={busyRowId === environment.id}
                          onClick={() => openRePair(environment)}
                        >
                          {environment.status === "pending_add"
                            ? "Re-pair to finish"
                            : "Re-pair"}
                        </Button>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button
                              type="button"
                              variant="ghost"
                              size="icon-sm"
                              data-testid={`connections-remove-${environment.id}`}
                              aria-label={`Remove ${environment.name}`}
                              disabled={busyRowId === environment.id}
                              onClick={() => setRemoving(environment)}
                              className="text-[var(--status-error)] hover:text-[var(--status-error)]"
                            >
                              {busyRowId === environment.id ? (
                                <Loader2 className="h-4 w-4 animate-spin" />
                              ) : (
                                <Trash2 className="h-4 w-4" />
                              )}
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>Remove environment</TooltipContent>
                        </Tooltip>
                      </div>
                    )}
                  </li>
                );
              })}
            </ul>
          )}

          <div className="pt-1">
            <p className="text-xs font-medium text-[var(--text-secondary)]">
              This Mac
            </p>
            <p
              className="text-xs text-[var(--text-muted)]"
              data-testid="connections-local"
            >
              Local environment — always available.
            </p>
          </div>
        </div>
      </Card>

      <AddEnvironmentDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        {...(rePairTarget !== null
          ? { lockedHost: rePairTarget.baseUrl, initialName: rePairTarget.name }
          : {})}
        onPaired={() => {
          void refresh();
        }}
      />

      <AlertDialog
        open={removing !== null}
        onOpenChange={(open) => {
          if (!open) {
            setRemoving(null);
          }
        }}
      >
        <AlertDialogContent data-testid="connections-remove-confirm">
          <AlertDialogHeader>
            <AlertDialogTitle>
              Remove {removing?.name ?? "this environment"}?
            </AlertDialogTitle>
            <AlertDialogDescription>
              This Mac's credential is revoked on the host and deleted from the
              Keychain, and this environment's local view state is cleared.
              Removal finishes in the background and resumes at next launch if
              interrupted. Pair again to restore access.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel data-testid="connections-remove-cancel">
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction
              data-testid="connections-remove-confirm-action"
              onClick={() => {
                if (removing !== null) {
                  void handleRemove(removing);
                }
                setRemoving(null);
              }}
              className="bg-[var(--status-error)] text-white hover:bg-[var(--status-error)]"
            >
              Remove environment
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
