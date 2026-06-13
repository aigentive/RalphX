use std::collections::BTreeMap;

use crate::domain::{entities::ideation::VerificationRoundSnapshot, services::gap_fingerprint};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanModeVerdict {
    Accepted,
    Declined,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanModeVerdictCaptureInput {
    pub project_id: String,
    pub conversation_id: String,
    pub planning_session_id: Option<String>,
    pub accepted_session_id: Option<String>,
    pub plan_artifact_id: Option<String>,
    pub verdict: PlanModeVerdict,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanModeVerdictOutcome {
    pub project_id: String,
    pub source: String,
    pub outcome_class: String,
    pub status: String,
    pub refs: BTreeMap<String, String>,
    pub evidence_summary: String,
    pub mutates_accepted_session: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LearnedSkillStatus {
    Staged,
    Approved,
    Rejected,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LearnedSkillStage {
    Planning,
    Verification,
    Review,
    Execution,
    Merge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LearnedSkillBucket {
    Planning,
    Verification,
    Review,
    Execution,
    Merge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedSkillRecord {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub status: LearnedSkillStatus,
    pub caller_surfaces: Vec<String>,
    pub stages: Vec<LearnedSkillStage>,
    pub buckets: Vec<LearnedSkillBucket>,
    pub path_scopes: Vec<String>,
    pub compact_guidance: String,
    pub predicted_effect: String,
    pub provenance_refs: Vec<String>,
}

impl LearnedSkillRecord {
    pub fn with_caller_surfaces(mut self, caller_surfaces: Vec<&str>) -> Self {
        self.caller_surfaces = caller_surfaces.into_iter().map(str::to_string).collect();
        self
    }

    pub fn with_stages(mut self, stages: Vec<LearnedSkillStage>) -> Self {
        self.stages = stages;
        self
    }

    pub fn with_buckets(mut self, buckets: Vec<LearnedSkillBucket>) -> Self {
        self.buckets = buckets;
        self
    }

    pub fn with_path_scopes(mut self, path_scopes: Vec<&str>) -> Self {
        self.path_scopes = path_scopes.into_iter().map(str::to_string).collect();
        self
    }

    pub fn with_predicted_effect(mut self, predicted_effect: &str) -> Self {
        self.predicted_effect = predicted_effect.to_string();
        self
    }

    pub fn with_provenance_refs(mut self, provenance_refs: Vec<&str>) -> Self {
        self.provenance_refs = provenance_refs.into_iter().map(str::to_string).collect();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedSkillSelectionRequest {
    pub project_id: String,
    pub caller_surface: String,
    pub stage: LearnedSkillStage,
    pub bucket: LearnedSkillBucket,
    pub touched_paths: Vec<String>,
    pub max_skills: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedSkillConstraintCitation {
    pub skill_id: String,
    pub title: String,
    pub predicted_effect: String,
    pub compact_guidance: String,
    pub provenance_refs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerificationGapRecurrenceGate {
    pub min_occurrences: usize,
    pub min_rounds: usize,
    pub min_corpus_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationGapRecurrenceReport {
    pub corpus_size: usize,
    pub recurring_gaps: Vec<VerificationGapRecurrence>,
    pub suppressed_gaps: Vec<SuppressedVerificationGapRecurrence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationGapRecurrence {
    pub fingerprint: String,
    pub occurrences: usize,
    pub distinct_rounds: usize,
    pub descriptions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuppressedVerificationGapRecurrence {
    pub fingerprint: String,
    pub occurrences: usize,
    pub distinct_rounds: usize,
    pub reason: VerificationGapSuppressionReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationGapSuppressionReason {
    BelowCorpusGate,
    BelowOccurrenceGate,
    BelowRoundGate,
}

pub fn capture_plan_mode_verdict(
    input: PlanModeVerdictCaptureInput,
) -> Option<PlanModeVerdictOutcome> {
    if input.verdict == PlanModeVerdict::Skipped {
        return None;
    }
    let planning_session_id = input.planning_session_id.as_ref()?;

    let mut refs = BTreeMap::from([
        ("conversation_id".to_string(), input.conversation_id.clone()),
        (
            "planning_session_id".to_string(),
            planning_session_id.clone(),
        ),
    ]);
    if let Some(accepted_session_id) = input.accepted_session_id.as_deref() {
        refs.insert(
            "accepted_session_id".to_string(),
            accepted_session_id.to_string(),
        );
    }
    if let Some(plan_artifact_id) = input.plan_artifact_id.as_deref() {
        refs.insert("plan_artifact_id".to_string(), plan_artifact_id.to_string());
    }

    let outcome_class = match input.verdict {
        PlanModeVerdict::Accepted => "plan_mode_accepted",
        PlanModeVerdict::Declined => "plan_mode_declined",
        PlanModeVerdict::Skipped => unreachable!("skipped verdicts return before capture"),
    };

    Some(PlanModeVerdictOutcome {
        project_id: input.project_id,
        source: "plan_mode".to_string(),
        outcome_class: outcome_class.to_string(),
        status: "eligible".to_string(),
        refs,
        evidence_summary: compact_summary(input.reason.as_deref()),
        mutates_accepted_session: false,
    })
}

pub fn select_pre_execution_learned_skills(
    request: LearnedSkillSelectionRequest,
    skills: &[LearnedSkillRecord],
) -> Vec<LearnedSkillRecord> {
    if request.max_skills == 0 {
        return Vec::new();
    }

    let mut selected = skills
        .iter()
        .filter(|skill| skill.status == LearnedSkillStatus::Approved)
        .filter(|skill| skill.project_id == request.project_id)
        .filter(|skill| caller_surface_matches(&request.caller_surface, &skill.caller_surfaces))
        .filter(|skill| skill.stages.contains(&request.stage))
        .filter(|skill| skill.buckets.contains(&request.bucket))
        .filter(|skill| path_scope_matches(&request.touched_paths, &skill.path_scopes))
        .cloned()
        .collect::<Vec<_>>();
    selected.sort_by(|left, right| left.id.cmp(&right.id));
    selected.truncate(request.max_skills);
    selected
}

fn caller_surface_matches(caller_surface: &str, allowed_surfaces: &[String]) -> bool {
    let caller_surface = caller_surface.trim();
    if caller_surface.is_empty() || allowed_surfaces.is_empty() {
        return false;
    }

    allowed_surfaces
        .iter()
        .any(|surface| surface.trim() == caller_surface)
}

pub fn build_constraint_bundle_skill_citations(
    selected_skills: &[LearnedSkillRecord],
) -> Vec<LearnedSkillConstraintCitation> {
    selected_skills
        .iter()
        .map(|skill| LearnedSkillConstraintCitation {
            skill_id: skill.id.clone(),
            title: skill.title.clone(),
            predicted_effect: skill.predicted_effect.clone(),
            compact_guidance: skill.compact_guidance.clone(),
            provenance_refs: skill.provenance_refs.clone(),
        })
        .collect()
}

pub fn verification_gap_recurrence_candidates(
    rounds: &[VerificationRoundSnapshot],
    gate: VerificationGapRecurrenceGate,
) -> VerificationGapRecurrenceReport {
    let mut occurrences = BTreeMap::<String, FingerprintEvidence>::new();
    for round in rounds {
        for (index, gap) in round.gaps.iter().enumerate() {
            let fingerprint = round
                .fingerprints
                .get(index)
                .cloned()
                .unwrap_or_else(|| gap_fingerprint(&gap.description));
            let evidence = occurrences
                .entry(fingerprint.clone())
                .or_insert_with(|| FingerprintEvidence::new(fingerprint));
            evidence.record(round.round, &gap.description);
        }
    }

    let corpus_size = occurrences
        .values()
        .map(|evidence| evidence.occurrences)
        .sum::<usize>();
    let mut recurring_gaps = Vec::new();
    let mut suppressed_gaps = Vec::new();
    for evidence in occurrences.into_values() {
        if corpus_size < gate.min_corpus_size {
            suppressed_gaps
                .push(evidence.suppress(VerificationGapSuppressionReason::BelowCorpusGate));
            continue;
        }
        if evidence.occurrences < gate.min_occurrences {
            suppressed_gaps
                .push(evidence.suppress(VerificationGapSuppressionReason::BelowOccurrenceGate));
            continue;
        }
        if evidence.distinct_rounds() < gate.min_rounds {
            suppressed_gaps
                .push(evidence.suppress(VerificationGapSuppressionReason::BelowRoundGate));
            continue;
        }
        recurring_gaps.push(evidence.promote());
    }

    VerificationGapRecurrenceReport {
        corpus_size,
        recurring_gaps,
        suppressed_gaps,
    }
}

fn compact_summary(reason: Option<&str>) -> String {
    const MAX_SUMMARY_CHARS: usize = 240;
    let reason = reason.map(str::trim).filter(|value| !value.is_empty());
    let Some(reason) = reason else {
        return "Plan-mode verdict captured without model transcript body.".to_string();
    };
    if reason.chars().count() <= MAX_SUMMARY_CHARS {
        return reason.to_string();
    }

    let mut summary = reason.chars().take(MAX_SUMMARY_CHARS).collect::<String>();
    summary.push_str("...");
    summary
}

fn path_scope_matches(touched_paths: &[String], path_scopes: &[String]) -> bool {
    if path_scopes.is_empty() {
        return true;
    }

    touched_paths.iter().any(|path| {
        let Some(path) = normalize_relative_path(path) else {
            return false;
        };
        path_scopes.iter().any(|scope| {
            normalize_relative_path(scope)
                .map(|scope| path == scope || path.starts_with(&format!("{scope}/")))
                .unwrap_or(false)
        })
    })
}

fn normalize_relative_path(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.contains('\\') {
        return None;
    }
    let trimmed = trimmed.trim_end_matches('/');
    if trimmed.is_empty()
        || trimmed.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.contains(std::path::MAIN_SEPARATOR)
        })
    {
        return None;
    }
    Some(trimmed.to_string())
}

#[derive(Debug)]
struct FingerprintEvidence {
    fingerprint: String,
    occurrences: usize,
    round_counts: BTreeMap<u32, usize>,
    descriptions: Vec<String>,
}

impl FingerprintEvidence {
    fn new(fingerprint: String) -> Self {
        Self {
            fingerprint,
            occurrences: 0,
            round_counts: BTreeMap::new(),
            descriptions: Vec::new(),
        }
    }

    fn record(&mut self, round: u32, description: &str) {
        self.occurrences += 1;
        *self.round_counts.entry(round).or_insert(0) += 1;
        let description = description.trim();
        if !description.is_empty()
            && !self
                .descriptions
                .iter()
                .any(|existing| existing == description)
        {
            self.descriptions.push(description.to_string());
        }
    }

    fn distinct_rounds(&self) -> usize {
        self.round_counts.len()
    }

    fn suppress(
        self,
        reason: VerificationGapSuppressionReason,
    ) -> SuppressedVerificationGapRecurrence {
        SuppressedVerificationGapRecurrence {
            fingerprint: self.fingerprint,
            occurrences: self.occurrences,
            distinct_rounds: self.round_counts.len(),
            reason,
        }
    }

    fn promote(self) -> VerificationGapRecurrence {
        VerificationGapRecurrence {
            fingerprint: self.fingerprint,
            occurrences: self.occurrences,
            distinct_rounds: self.round_counts.len(),
            descriptions: self.descriptions,
        }
    }
}
