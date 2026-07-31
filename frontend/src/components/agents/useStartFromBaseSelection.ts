import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type {
  AgentConversationBaseSelection,
  AgentConversationBranchMode,
  AgentConversationWorkspaceMode,

} from "@/api/chat";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import {
  fallbackBranchBaseOptions,
  loadBranchBaseOptions,
  loadPullRequestBaseOptions,
  synthesizeLocalBranchOption,
  type BranchBaseOption,
} from "@/components/shared/branchBaseOptions";

const LOCAL_BRANCH_KEY_PREFIX = "local_branch:";
const PULL_REQUEST_KEY_PREFIX = "pull_request:";

export interface UseStartFromBaseSelectionInput {
  activeProjectId: string | null;
  activeProjectBaseBranch: string | null;
  activeProjectWorkingDirectory: string | null;
  clickupTicketToken: string | null;
  mode: AgentConversationWorkspaceMode;
  onClearError: () => void;
}

export type StartFromBaseResolution =
  | {
      kind: "ok";
      base:
        | (AgentConversationBaseSelection & {
            branchMode: AgentConversationBranchMode;
          })
        | null;
    }
  | { kind: "unavailable"; base: null; unavailableRef: string };

export interface UseStartFromBaseSelectionResult {
  startFromOptions: BranchBaseOption[];
  pullRequestStartFromOptions: BranchBaseOption[];
  selectedStartFromKey: string;
  selectedStartFromSelection: AgentConversationBaseSelection | null;
  isLoadingStartFrom: boolean;
  isLoadingPullRequestStartFrom: boolean;
  pullRequestStartFromMessage: string | null;
  effectiveIsolatedBranch: boolean;
  forcesIsolatedBranch: boolean;
  handleStartFromChange: (key: string) => void;
  setIsolatedBranch: (value: boolean) => void;
  forceIsolatedBranch: () => void;
  resetExplicitSelection: () => void;
  ensureStartFromOptionsLoaded: () => void;
  searchPullRequestStartFromOptions: (query: string) => void;
  resolveBaseForSubmit: () => Promise<StartFromBaseResolution>;
}

export function useStartFromBaseSelection({
  activeProjectId,
  activeProjectBaseBranch,
  activeProjectWorkingDirectory,
  clickupTicketToken,
  mode,
  onClearError,
}: UseStartFromBaseSelectionInput): UseStartFromBaseSelectionResult {
  const [startFromOptions, setStartFromOptions] = useState<BranchBaseOption[]>([]);
  const [pullRequestStartFromOptions, setPullRequestStartFromOptions] = useState<
    BranchBaseOption[]
  >([]);
  const [selectedStartFromKey, setSelectedStartFromKey] = useState("");
  const [isStartFromIsolatedBranch, setIsStartFromIsolatedBranch] =
    useState(false);
  const [isLoadingStartFrom, setIsLoadingStartFrom] = useState(false);
  const [isLoadingPullRequestStartFrom, setIsLoadingPullRequestStartFrom] = useState(false);
  const [pullRequestStartFromMessage, setPullRequestStartFromMessage] =
    useState<string | null>(null);
  const [hydratedStartFromProjectId, setHydratedStartFromProjectId] =
    useState<string | null>(null);
  const startFromRequestRef = useRef(0);
  const pullRequestStartFromRequestRef = useRef(0);
  const userSelectedStartFromRef = useRef(false);
  /** Live mirrors so resolve-time reconciliation never reads a stale render value (§I2). */
  const selectedStartFromKeyRef = useRef("");
  const selectedStartFromOptionRef = useRef<BranchBaseOption | null>(null);
  const hydratedProjectIdRef = useRef<string | null>(null);
  const setBranchBaseCacheForProject = useAgentSessionStore(
    (s) => s.setBranchBaseCacheForProject
  );
  const setLastBranchBaseSelectionForProject = useAgentSessionStore(
    (s) => s.setLastBranchBaseSelectionForProject
  );

  /**
   * Single writer for the selected key and its isolation state (§I5). Nothing
   * else in this hook may call `setSelectedStartFromKey` directly.
   */
  const applySelection = useCallback(
    (
      nextKey: string,
      options: BranchBaseOption[],
      { userInitiated }: { userInitiated: boolean }
    ) => {
      if (userInitiated) {
        userSelectedStartFromRef.current = true;
      }
      const nextOption = options.find((option) => option.key === nextKey) ?? null;
      selectedStartFromKeyRef.current = nextKey;
      selectedStartFromOptionRef.current = nextOption;
      setSelectedStartFromKey(nextKey);
      setIsStartFromIsolatedBranch(
        startSelectionDefaultsToIsolatedBranch(nextOption?.selection ?? null)
      );
    },
    []
  );

  const setIsolatedBranch = useCallback((value: boolean) => {
    setIsStartFromIsolatedBranch(value);
  }, []);

  const forceIsolatedBranch = useCallback(() => {
    setIsStartFromIsolatedBranch(true);
  }, []);

  /**
   * Drops explicit intent (project switch, draft prefill) and re-derives
   * isolation from the selection actually in effect (§I5). It must not force
   * `false`, or it would contradict a selection the hydration pass already
   * applied.
   */
  const resetExplicitSelection = useCallback(() => {
    userSelectedStartFromRef.current = false;
    setIsStartFromIsolatedBranch(
      startSelectionDefaultsToIsolatedBranch(
        selectedStartFromOptionRef.current?.selection ?? null
      )
    );
  }, []);

  const allStartFromOptions = useMemo(
    () => [...startFromOptions, ...pullRequestStartFromOptions],
    [pullRequestStartFromOptions, startFromOptions]
  );
  const selectedStartFrom =
    allStartFromOptions.find((option) => option.key === selectedStartFromKey) ?? null;
  const fallbackStartFrom = useMemo<AgentConversationBaseSelection | null>(() => {
    if (!activeProjectId) {
      return null;
    }
    const ref = activeProjectBaseBranch ?? "main";
    return {
      kind: "project_default",
      ref,
      displayName: `Project default (${ref})`,
    };
  }, [activeProjectBaseBranch, activeProjectId]);
  const selectedStartFromSelection =
    selectedStartFrom?.selection ?? fallbackStartFrom;
  const selectionForcesIsolatedBranch =
    selectedStartFromSelection
      ? startSelectionForcesIsolatedBranch(selectedStartFromSelection)
      : false;
  const startFromForcesIsolatedBranch =
    mode === "review_pr" || selectionForcesIsolatedBranch;
  const effectiveStartFromIsolatedBranch =
    startFromForcesIsolatedBranch || isStartFromIsolatedBranch;
  const handleStartFromChange = useCallback(
    (nextKey: string) => {
      onClearError();
      applySelection(nextKey, allStartFromOptions, { userInitiated: true });
      if (activeProjectId && !isTransientStartFromKey(nextKey)) {
        setLastBranchBaseSelectionForProject(activeProjectId, nextKey);
      }
    },
    [
      activeProjectId,
      allStartFromOptions,
      applySelection,
      onClearError,
      setLastBranchBaseSelectionForProject,
    ]
  );
  useEffect(() => {
    // §RC5: only a genuine project change may discard explicit intent. A
    // baseBranch/workingDirectory churn on the same project must not.
    const projectChanged = hydratedProjectIdRef.current !== activeProjectId;
    hydratedProjectIdRef.current = activeProjectId;
    const preserveExplicitSelection =
      !projectChanged &&
      userSelectedStartFromRef.current &&
      !isTransientStartFromKey(selectedStartFromKeyRef.current);

    startFromRequestRef.current += 1;
    pullRequestStartFromRequestRef.current += 1;
    setHydratedStartFromProjectId(null);
    setPullRequestStartFromOptions([]);
    setPullRequestStartFromMessage(null);
    setIsLoadingPullRequestStartFrom(false);
    if (!preserveExplicitSelection) {
      setIsStartFromIsolatedBranch(false);
      userSelectedStartFromRef.current = false;
    }

    if (!activeProjectId || !activeProjectWorkingDirectory) {
      setStartFromOptions([]);
      applySelection("", [], { userInitiated: false });
      setIsLoadingStartFrom(false);
      return;
    }

    const {
      branchBaseCacheByProjectId: cachedBranchBaseByProjectId,
      lastBranchBaseSelectionByProjectId: rememberedBranchBaseByProjectId,
    } = useAgentSessionStore.getState();
    const fallback = fallbackBranchBaseOptions(activeProjectBaseBranch);
    const cached = cachedBranchBaseByProjectId[activeProjectId];
    const options = cached?.options.length ? cached.options : fallback.options;
    const preferredKey =
      rememberedBranchBaseByProjectId[activeProjectId] ??
      cached?.selectedKey ??
      fallback.selectedKey;
    const nextSelectedStartFromKey =
      resolveBranchSelectionKey(options, preferredKey) ??
      resolveBranchSelectionKey(options, fallback.selectedKey) ??
      fallback.selectedKey;
    setStartFromOptions(options);
    if (!preserveExplicitSelection) {
      applySelection(nextSelectedStartFromKey, options, { userInitiated: false });
    }
    setIsLoadingStartFrom(false);
  }, [
    activeProjectBaseBranch,
    activeProjectId,
    activeProjectWorkingDirectory,
    applySelection,
  ]);
  const searchPullRequestStartFromOptions = useCallback(
    (query: string) => {
      if (!activeProjectId) {
        setPullRequestStartFromOptions([]);
        setPullRequestStartFromMessage(null);
        setIsLoadingPullRequestStartFrom(false);
        return;
      }

      const requestId = ++pullRequestStartFromRequestRef.current;
      setIsLoadingPullRequestStartFrom(true);
      setPullRequestStartFromMessage(null);

      void loadPullRequestBaseOptions({ projectId: activeProjectId, query })
        .then((options) => {
          if (pullRequestStartFromRequestRef.current !== requestId) {
            return;
          }
          setPullRequestStartFromOptions((current) => {
            const selected = current.find(
              (option) => option.key === selectedStartFromKey
            );
            if (
              selected &&
              !options.some((option) => option.key === selected.key)
            ) {
              return [selected, ...options];
            }
            return options;
          });
          setIsLoadingPullRequestStartFrom(false);
        })
        .catch((err) => {
          if (pullRequestStartFromRequestRef.current !== requestId) {
            return;
          }
          setPullRequestStartFromOptions((current) =>
            current.filter((option) => option.key === selectedStartFromKey)
          );
          setPullRequestStartFromMessage(
            err instanceof Error
              ? err.message
              : "Unable to search pull requests"
          );
          setIsLoadingPullRequestStartFrom(false);
        });
    },
    [activeProjectId, selectedStartFromKey]
  );
  const ensureStartFromOptionsLoaded = useCallback(() => {
    if (
      !activeProjectId ||
      !activeProjectWorkingDirectory ||
      isLoadingStartFrom ||
      hydratedStartFromProjectId === activeProjectId
    ) {
      return;
    }

    const requestId = ++startFromRequestRef.current;
    setIsLoadingStartFrom(true);

    void loadBranchBaseOptions({
      projectId: activeProjectId,
      workingDirectory: activeProjectWorkingDirectory,
      projectBaseBranch: activeProjectBaseBranch,
    })
      .then((result) => {
        if (startFromRequestRef.current !== requestId) {
          return;
        }

        // §I2: reconcile against live state, never the closure snapshot that
        // existed when this callback was created.
        const live = useAgentSessionStore.getState();
        const liveRemembered =
          live.lastBranchBaseSelectionByProjectId[activeProjectId] ?? null;
        const currentKey = selectedStartFromKeyRef.current;

        // §I3: a degraded sub-load is not evidence that a branch is gone.
        // Re-admit an explicitly selected ref that git still reports.
        let nextOptions = result.options;
        if (
          userSelectedStartFromRef.current &&
          currentKey.startsWith(LOCAL_BRANCH_KEY_PREFIX) &&
          !nextOptions.some((option) => option.key === currentKey)
        ) {
          const ref = currentKey.slice(LOCAL_BRANCH_KEY_PREFIX.length);
          const degraded = result.degraded;
          if (
            Boolean(degraded?.agentBranches || degraded?.planBranches) &&
            (result.knownBranchRefs ?? []).includes(ref)
          ) {
            const previousLabel = selectedStartFromOptionRef.current?.label;
            nextOptions = [
              ...nextOptions,
              synthesizeLocalBranchOption(ref, previousLabel),
            ];
          }
        }
        setStartFromOptions(nextOptions);

        // §I1: an explicit pick outranks any refresh that lands after it.
        // Cache the refreshed list, but keep the key the user chose.
        if (userSelectedStartFromRef.current) {
          selectedStartFromOptionRef.current =
            nextOptions.find((option) => option.key === currentKey) ??
            selectedStartFromOptionRef.current;
          if (!isTransientStartFromKey(currentKey)) {
            setBranchBaseCacheForProject(activeProjectId, nextOptions, currentKey);
          }
          setHydratedStartFromProjectId(activeProjectId);
          setIsLoadingStartFrom(false);
          return;
        }

        const normalSelectedKey =
          resolveBranchSelectionKey(nextOptions, liveRemembered) ??
          resolveBranchSelectionKey(nextOptions, currentKey) ??
          resolveBranchSelectionKey(nextOptions, result.selectedKey) ??
          result.selectedKey;
        const clickupCandidates = clickupTicketToken
          ? nextOptions.filter(
              (option) =>
                option.selection.kind === "local_branch" &&
                containsTicketToken(option.selection.ref, clickupTicketToken),
            )
          : [];
        const clickupSelectedKey =
          clickupCandidates.length === 1 ? clickupCandidates[0]?.key : null;
        const nextBranchSelectedKey = clickupSelectedKey ?? normalSelectedKey;
        if (!isTransientStartFromKey(currentKey)) {
          applySelection(nextBranchSelectedKey, nextOptions, {
            userInitiated: false,
          });
          if (clickupSelectedKey) {
            setIsStartFromIsolatedBranch(false);
          }
        }
        setBranchBaseCacheForProject(activeProjectId, nextOptions, normalSelectedKey);
        if (!clickupSelectedKey) {
          setLastBranchBaseSelectionForProject(activeProjectId, nextBranchSelectedKey);
        }
        setHydratedStartFromProjectId(activeProjectId);
        setIsLoadingStartFrom(false);
      })
      .catch(() => {
        if (startFromRequestRef.current !== requestId) {
          return;
        }
        setHydratedStartFromProjectId(activeProjectId);
        setIsLoadingStartFrom(false);
      });
  }, [
    activeProjectBaseBranch,
    activeProjectId,
    activeProjectWorkingDirectory,
    applySelection,
    clickupTicketToken,
    hydratedStartFromProjectId,
    isLoadingStartFrom,
    setBranchBaseCacheForProject,
    setLastBranchBaseSelectionForProject,
  ]);

  useEffect(() => {
    if (clickupTicketToken) {
      ensureStartFromOptionsLoaded();
    }
  }, [clickupTicketToken, ensureStartFromOptionsLoaded]);

  /**
   * §I4: submit never substitutes a base the user did not choose. An explicit
   * pick that no longer resolves gets exactly one fresh load; only a second
   * miss blocks the start.
   */
  const resolveBaseForSubmit =
    useCallback<UseStartFromBaseSelectionResult["resolveBaseForSubmit"]>(async () => {
      const withBranchMode = (selection: AgentConversationBaseSelection) => ({
        ...selection,
        branchMode: branchModeForStartSelection(
          selection,
          effectiveStartFromIsolatedBranch
        ),
      });

      const direct =
        allStartFromOptions.find((option) => option.key === selectedStartFromKey) ??
        null;
      if (direct) {
        return { kind: "ok", base: withBranchMode(direct.selection) };
      }

      // No explicit pick: the project default is a legitimate default, not a
      // substitution.
      if (!userSelectedStartFromRef.current) {
        return {
          kind: "ok",
          base: fallbackStartFrom ? withBranchMode(fallbackStartFrom) : null,
        };
      }

      const wantedRef = refFromStartFromKey(selectedStartFromKey);
      if (!activeProjectId || !activeProjectWorkingDirectory) {
        return { kind: "unavailable", base: null, unavailableRef: wantedRef };
      }

      const requestId = ++startFromRequestRef.current;
      setIsLoadingStartFrom(true);
      try {
        const retry = await loadBranchBaseOptions({
          projectId: activeProjectId,
          workingDirectory: activeProjectWorkingDirectory,
          projectBaseBranch: activeProjectBaseBranch,
        });
        // Match order: exact key -> same ref under a changed label -> a ref git
        // still reports that a degraded sub-load dropped.
        const rematch =
          retry.options.find((option) => option.key === selectedStartFromKey) ??
          retry.options.find((option) => option.selection.ref === wantedRef) ??
          ((retry.knownBranchRefs ?? []).includes(wantedRef)
            ? synthesizeLocalBranchOption(wantedRef)
            : null);
        if (startFromRequestRef.current === requestId) {
          setStartFromOptions(
            rematch && !retry.options.includes(rematch)
              ? [...retry.options, rematch]
              : retry.options
          );
        }
        if (rematch) {
          return { kind: "ok", base: withBranchMode(rematch.selection) };
        }
        return { kind: "unavailable", base: null, unavailableRef: wantedRef };
      } catch {
        // A failed retry is not proof of absence, but it is not proof of
        // presence either. Block rather than substitute.
        return { kind: "unavailable", base: null, unavailableRef: wantedRef };
      } finally {
        if (startFromRequestRef.current === requestId) {
          setIsLoadingStartFrom(false);
        }
      }
    }, [
      activeProjectBaseBranch,
      activeProjectId,
      activeProjectWorkingDirectory,
      allStartFromOptions,
      effectiveStartFromIsolatedBranch,
      fallbackStartFrom,
      selectedStartFromKey,
    ]);

  return {
    startFromOptions,
    pullRequestStartFromOptions,
    selectedStartFromKey,
    selectedStartFromSelection,
    isLoadingStartFrom,
    isLoadingPullRequestStartFrom,
    pullRequestStartFromMessage,
    effectiveIsolatedBranch: effectiveStartFromIsolatedBranch,
    forcesIsolatedBranch: startFromForcesIsolatedBranch,
    handleStartFromChange,
    setIsolatedBranch,
    forceIsolatedBranch,
    resetExplicitSelection,
    ensureStartFromOptionsLoaded,
    searchPullRequestStartFromOptions,
    resolveBaseForSubmit,
  };
}

function resolveBranchSelectionKey(
  options: BranchBaseOption[],
  preferredKey: string | null | undefined
) {
  if (!preferredKey) {
    return null;
  }
  return options.some((option) => option.key === preferredKey) ? preferredKey : null;
}

function branchModeForStartSelection(
  selection: AgentConversationBaseSelection,
  isIsolated: boolean
): AgentConversationBranchMode {
  if (selection.kind === "project_default") {
    return "isolated";
  }
  if (selection.kind === "current_branch") {
    return "isolated";
  }
  return isIsolated ? "isolated" : "linked";
}

function startSelectionForcesIsolatedBranch(
  selection: AgentConversationBaseSelection
): boolean {
  return selection.kind === "project_default" || selection.kind === "current_branch";
}

function startSelectionDefaultsToIsolatedBranch(
  selection: AgentConversationBaseSelection | null
): boolean {
  return selection !== null;
}

function isTransientStartFromKey(key: string) {
  return key.startsWith(PULL_REQUEST_KEY_PREFIX);
}

/** Recovers the git ref a start-from key points at, for user-facing errors and rematching. */
function refFromStartFromKey(key: string): string {
  if (key.startsWith(PULL_REQUEST_KEY_PREFIX)) {
    const rest = key.slice(PULL_REQUEST_KEY_PREFIX.length);
    const separator = rest.indexOf(":");
    return separator >= 0 ? rest.slice(separator + 1) : rest;
  }
  const separator = key.indexOf(":");
  return separator >= 0 ? key.slice(separator + 1) : key;
}
function containsTicketToken(value: string, token: string): boolean {
  const lowerValue = value.toLowerCase();
  const lowerToken = token.toLowerCase();
  let start = lowerValue.indexOf(lowerToken);
  while (start >= 0) {
    const before = start === 0 ? "" : lowerValue[start - 1];
    const after = lowerValue[start + lowerToken.length] ?? "";
    if (!/[a-z0-9]/.test(before ?? "") && !/[a-z0-9]/.test(after)) {
      return true;
    }
    start = lowerValue.indexOf(lowerToken, start + 1);
  }
  return false;
}
