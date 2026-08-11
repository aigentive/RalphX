export type VerificationRoundGap = {
  severity: string;
  category: string;
  description: string;
  why_it_matters?: string | null;
  source?: string | null;
  lens?: string | null;
};

export type VerificationRoundSummary = {
  artifact_id: string;
  title: string;
  created_at: string;
  author_teammate?: string | null;
  critic: string;
  round: number;
  status: string;
  coverage?: string | null;
  summary: string;
  gaps: VerificationRoundGap[];
};

export type VerificationRoundFindingMatch = {
  critic: string;
  found: boolean;
  total_matches: number;
  finding?: VerificationRoundSummary;
};

export type VerificationRoundDelegateInput = {
  job_id: string;
  critic: string;
  required?: boolean;
  label?: string;
};

export type VerificationRoundDelegateSnapshot = {
  job_id: string;
  status?: string;
  error?: string | null;
  delegated_status?: {
    agent_state?: {
      estimated_status?: string | null;
    };
    latest_run?: {
      status?: string | null;
      error_message?: string | null;
    } | null;
  } | null;
};

type DelegateAssessmentKind =
  | "finding_published"
  | "pending"
  | "infra_failure";

export type VerificationRoundDelegateAssessment = {
  job_id: string;
  label: string;
  critic: string;
  required: boolean;
  finding_found: boolean;
  assessment: DelegateAssessmentKind;
  status: string;
  reason: string;
};

export type VerificationRoundAssessment = {
  classification: "complete" | "pending" | "infra_failure";
  recommended_next_action:
    | "continue_round_analysis"
    | "perform_single_rescue_or_wait"
    | "complete_verification_with_infra_failure";
  summary: string;
  missing_required_critics: string[];
  delegate_assessments: VerificationRoundDelegateAssessment[];
  findings_by_critic: VerificationRoundFindingMatch[];
};

export type ParsedVerificationGap = {
  severity: "critical" | "high" | "medium" | "low";
  category: string;
  description: string;
  why_it_matters?: string;
  source?: "layer1" | "layer2" | "both";
};

export type ParsedVerificationCriticArtifact = {
  prefix: string;
  label: string;
  usable: boolean;
  artifact_id?: string;
  artifact_name?: string;
  artifact_created_at?: string;
  status?: string;
  critic?: string;
  round?: number;
  coverage?: string;
  summary?: string;
  gaps: ParsedVerificationGap[];
  parse_error?: string;
};

export type VerificationGapCounts = {
  critical: number;
  high: number;
  medium: number;
  low: number;
};

function mergeGapSource(
  current: ParsedVerificationGap["source"],
  incoming: ParsedVerificationGap["source"]
): ParsedVerificationGap["source"] {
  if (!current) {
    return incoming;
  }
  if (!incoming || current === incoming) {
    return current;
  }
  if (current === "both" || incoming === "both") {
    return "both";
  }
  return "both";
}

function gapDedupKey(gap: ParsedVerificationGap): string {
  return [
    gap.severity.trim().toLowerCase(),
    gap.category.trim().toLowerCase(),
    gap.description.trim().toLowerCase(),
    (gap.why_it_matters ?? "").trim().toLowerCase(),
  ].join("::");
}

export function aggregateVerificationGaps(findings: ParsedVerificationCriticArtifact[]): {
  merged_gaps: ParsedVerificationGap[];
  gap_counts: VerificationGapCounts;
} {
  const merged = new Map<string, ParsedVerificationGap>();

  for (const finding of findings) {
    if (!finding.usable) {
      continue;
    }

    for (const gap of finding.gaps) {
      const key = gapDedupKey(gap);
      const existing = merged.get(key);
      if (!existing) {
        merged.set(key, { ...gap });
        continue;
      }

      merged.set(key, {
        ...existing,
        why_it_matters: existing.why_it_matters ?? gap.why_it_matters,
        source: mergeGapSource(existing.source, gap.source),
      });
    }
  }

  const merged_gaps = Array.from(merged.values());
  const gap_counts: VerificationGapCounts = {
    critical: 0,
    high: 0,
    medium: 0,
    low: 0,
  };

  for (const gap of merged_gaps) {
    gap_counts[gap.severity] += 1;
  }

  return { merged_gaps, gap_counts };
}

function normalizeSeverity(value: unknown): ParsedVerificationGap["severity"] | null {
  if (value === "critical" || value === "high" || value === "medium" || value === "low") {
    return value;
  }
  return null;
}

function normalizeSource(value: unknown): ParsedVerificationGap["source"] | undefined {
  if (value === "layer1" || value === "layer2" || value === "both") {
    return value;
  }
  return undefined;
}

export function parseVerificationRoundSummary(params: {
  label: string;
  finding?: VerificationRoundSummary;
}): ParsedVerificationCriticArtifact {
  const finding = params.finding;
  const base: ParsedVerificationCriticArtifact = {
    prefix: params.label,
    label: params.label,
    usable: false,
    artifact_id: finding?.artifact_id,
    artifact_name: finding?.title,
    artifact_created_at: finding?.created_at,
    gaps: [],
  };

  if (!finding) {
    return {
      ...base,
      parse_error: "Typed verification finding is missing.",
    };
  }

  const status =
    finding.status === "complete" ||
    finding.status === "partial" ||
    finding.status === "error"
      ? finding.status
      : undefined;

  const gaps: ParsedVerificationGap[] = [];
  for (const gap of finding.gaps ?? []) {
    const severity = normalizeSeverity(gap.severity);
    const category = typeof gap.category === "string" ? gap.category.trim() : "";
    const description =
      typeof gap.description === "string" ? gap.description.trim() : "";
    if (!severity || category.length === 0 || description.length === 0) {
      continue;
    }
    gaps.push({
      severity,
      category,
      description,
      why_it_matters:
        typeof gap.why_it_matters === "string" ? gap.why_it_matters : undefined,
      source: normalizeSource(gap.source),
    });
  }

  const usable =
    typeof finding.critic === "string" &&
    typeof finding.summary === "string" &&
    status !== undefined;

  return {
    ...base,
    usable,
    status,
    critic: finding.critic,
    round: finding.round,
    coverage: typeof finding.coverage === "string" ? finding.coverage : undefined,
    summary: finding.summary,
    gaps,
    parse_error: usable ? undefined : "Typed verification finding is missing required fields.",
  };
}

function isRunningLike(status?: string | null): boolean {
  return (
    status === "running" ||
    status === "queued" ||
    status === "likely_generating" ||
    status === "likely_waiting"
  );
}

function classifyDelegate(
  delegate: VerificationRoundDelegateInput,
  findingMatch: VerificationRoundFindingMatch | undefined,
  snapshot: VerificationRoundDelegateSnapshot | undefined,
  rescueBudgetExhausted: boolean
): VerificationRoundDelegateAssessment {
  const label = delegate.label || delegate.critic.trim() || delegate.job_id;
  const required = delegate.required !== false;
  const findingFound = findingMatch?.found === true;

  if (findingFound) {
    return {
      job_id: delegate.job_id,
      label,
      critic: delegate.critic,
      required,
      finding_found: true,
      assessment: "finding_published",
      status: "completed",
      reason: "Required verification finding was published for this delegate.",
    };
  }

  const jobStatus = snapshot?.status ?? "unknown";
  const runStatus = snapshot?.delegated_status?.latest_run?.status ?? null;
  const estimatedStatus = snapshot?.delegated_status?.agent_state?.estimated_status ?? null;
  const error =
    snapshot?.error ??
    snapshot?.delegated_status?.latest_run?.error_message ??
    null;

  if (isRunningLike(jobStatus) || isRunningLike(runStatus) || isRunningLike(estimatedStatus)) {
    return {
      job_id: delegate.job_id,
      label,
      critic: delegate.critic,
      required,
      finding_found: false,
      assessment: "pending",
      status: jobStatus,
      reason:
        "Delegate still appears to be running and has not published the required verification finding yet.",
    };
  }

  const terminalWithoutFinding =
    jobStatus === "completed" ||
    jobStatus === "failed" ||
    jobStatus === "cancelled" ||
    runStatus === "completed" ||
    runStatus === "failed" ||
    runStatus === "cancelled";

  if (!rescueBudgetExhausted) {
    return {
      job_id: delegate.job_id,
      label,
      critic: delegate.critic,
      required,
      finding_found: false,
      assessment: "pending",
      status: jobStatus,
      reason:
        error ??
        (terminalWithoutFinding
          ? "Delegate finished without publishing the required verification finding yet; the bounded rescue/wait budget is still available."
          : "Required verification finding is still missing and the bounded rescue/wait budget is still available."),
    };
  }

  return {
    job_id: delegate.job_id,
    label,
    critic: delegate.critic,
    required,
    finding_found: false,
    assessment: "infra_failure",
    status: jobStatus,
    reason:
      error ??
      (terminalWithoutFinding
        ? "Delegate reached a terminal state without publishing the required verification finding."
        : "Required verification finding is still missing after the allowed wait/rescue budget."),
  };
}

export function assessVerificationRound(params: {
  delegates: VerificationRoundDelegateInput[];
  findingsByCritic: VerificationRoundFindingMatch[];
  delegateSnapshots: VerificationRoundDelegateSnapshot[];
  rescueBudgetExhausted?: boolean;
}): VerificationRoundAssessment {
  const rescueBudgetExhausted = params.rescueBudgetExhausted === true;
  const findingByCritic = new Map(
    params.findingsByCritic.map((findingMatch) => [findingMatch.critic, findingMatch] as const)
  );
  const snapshotByJobId = new Map(
    params.delegateSnapshots.map((snapshot) => [snapshot.job_id, snapshot] as const)
  );

  const delegateAssessments = params.delegates.map((delegate) =>
    classifyDelegate(
      delegate,
      findingByCritic.get(delegate.critic),
      snapshotByJobId.get(delegate.job_id),
      rescueBudgetExhausted
    )
  );

  const requiredAssessments = delegateAssessments.filter((delegate) => delegate.required);
  const missingRequiredCritics = requiredAssessments
    .filter((delegate) => !delegate.finding_found)
    .map((delegate) => delegate.critic);

  const classification =
    missingRequiredCritics.length === 0
      ? "complete"
      : requiredAssessments.some((delegate) => delegate.assessment === "infra_failure")
        ? "infra_failure"
        : "pending";

  const recommendedNextAction =
    classification === "complete"
      ? "continue_round_analysis"
      : classification === "pending"
        ? "perform_single_rescue_or_wait"
        : "complete_verification_with_infra_failure";

  const summary =
    classification === "complete"
      ? "All required verification findings were published."
      : classification === "pending"
        ? `Required verification findings are still pending for: ${missingRequiredCritics.join(", ")}.`
        : `Required verification findings are missing after the allowed wait/rescue budget: ${missingRequiredCritics.join(", ")}.`;

  return {
    classification,
    recommended_next_action: recommendedNextAction,
    summary,
    missing_required_critics: missingRequiredCritics,
    delegate_assessments: delegateAssessments,
    findings_by_critic: params.findingsByCritic,
  };
}
