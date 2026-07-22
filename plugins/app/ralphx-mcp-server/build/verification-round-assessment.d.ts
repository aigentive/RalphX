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
type DelegateAssessmentKind = "finding_published" | "pending" | "infra_failure";
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
    recommended_next_action: "continue_round_analysis" | "perform_single_rescue_or_wait" | "complete_verification_with_infra_failure";
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
export declare function aggregateVerificationGaps(findings: ParsedVerificationCriticArtifact[]): {
    merged_gaps: ParsedVerificationGap[];
    gap_counts: VerificationGapCounts;
};
export declare function parseVerificationRoundSummary(params: {
    label: string;
    finding?: VerificationRoundSummary;
}): ParsedVerificationCriticArtifact;
export declare function assessVerificationRound(params: {
    delegates: VerificationRoundDelegateInput[];
    findingsByCritic: VerificationRoundFindingMatch[];
    delegateSnapshots: VerificationRoundDelegateSnapshot[];
    rescueBudgetExhausted?: boolean;
}): VerificationRoundAssessment;
export {};
//# sourceMappingURL=verification-round-assessment.d.ts.map