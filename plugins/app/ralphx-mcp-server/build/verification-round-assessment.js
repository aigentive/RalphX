function mergeGapSource(current, incoming) {
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
function gapDedupKey(gap) {
    return [
        gap.severity.trim().toLowerCase(),
        gap.category.trim().toLowerCase(),
        gap.description.trim().toLowerCase(),
        (gap.why_it_matters ?? "").trim().toLowerCase(),
    ].join("::");
}
export function aggregateVerificationGaps(findings) {
    const merged = new Map();
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
    const gap_counts = {
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
function normalizeSeverity(value) {
    if (value === "critical" || value === "high" || value === "medium" || value === "low") {
        return value;
    }
    return null;
}
function normalizeSource(value) {
    if (value === "layer1" || value === "layer2" || value === "both") {
        return value;
    }
    return undefined;
}
export function parseVerificationRoundSummary(params) {
    const finding = params.finding;
    const base = {
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
    const status = finding.status === "complete" ||
        finding.status === "partial" ||
        finding.status === "error"
        ? finding.status
        : undefined;
    const gaps = [];
    for (const gap of finding.gaps ?? []) {
        const severity = normalizeSeverity(gap.severity);
        const category = typeof gap.category === "string" ? gap.category.trim() : "";
        const description = typeof gap.description === "string" ? gap.description.trim() : "";
        if (!severity || category.length === 0 || description.length === 0) {
            continue;
        }
        gaps.push({
            severity,
            category,
            description,
            why_it_matters: typeof gap.why_it_matters === "string" ? gap.why_it_matters : undefined,
            source: normalizeSource(gap.source),
        });
    }
    const usable = typeof finding.critic === "string" &&
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
function isRunningLike(status) {
    return (status === "running" ||
        status === "queued" ||
        status === "likely_generating" ||
        status === "likely_waiting");
}
function classifyDelegate(delegate, findingMatch, snapshot, rescueBudgetExhausted) {
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
    const error = snapshot?.error ??
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
            reason: "Delegate still appears to be running and has not published the required verification finding yet.",
        };
    }
    const terminalWithoutFinding = jobStatus === "completed" ||
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
            reason: error ??
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
        reason: error ??
            (terminalWithoutFinding
                ? "Delegate reached a terminal state without publishing the required verification finding."
                : "Required verification finding is still missing after the allowed wait/rescue budget."),
    };
}
export function assessVerificationRound(params) {
    const rescueBudgetExhausted = params.rescueBudgetExhausted === true;
    const findingByCritic = new Map(params.findingsByCritic.map((findingMatch) => [findingMatch.critic, findingMatch]));
    const snapshotByJobId = new Map(params.delegateSnapshots.map((snapshot) => [snapshot.job_id, snapshot]));
    const delegateAssessments = params.delegates.map((delegate) => classifyDelegate(delegate, findingByCritic.get(delegate.critic), snapshotByJobId.get(delegate.job_id), rescueBudgetExhausted));
    const requiredAssessments = delegateAssessments.filter((delegate) => delegate.required);
    const missingRequiredCritics = requiredAssessments
        .filter((delegate) => !delegate.finding_found)
        .map((delegate) => delegate.critic);
    const classification = missingRequiredCritics.length === 0
        ? "complete"
        : requiredAssessments.some((delegate) => delegate.assessment === "infra_failure")
            ? "infra_failure"
            : "pending";
    const recommendedNextAction = classification === "complete"
        ? "continue_round_analysis"
        : classification === "pending"
            ? "perform_single_rescue_or_wait"
            : "complete_verification_with_infra_failure";
    const summary = classification === "complete"
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
//# sourceMappingURL=verification-round-assessment.js.map