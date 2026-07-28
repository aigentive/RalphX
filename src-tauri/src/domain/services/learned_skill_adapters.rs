use std::collections::{BTreeMap, HashSet};

use crate::domain::{
    entities::{ideation::VerificationRoundSnapshot, ProjectSkill, ProjectSkillLifecycleStatus},
    services::gap_fingerprint,
};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearnedSkillStatus {
    Staged,
    Approved,
    Rejected,
    Stale,
    Archived,
    Retired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearnedSkillStage {
    Planning,
    Verification,
    Review,
    Execution,
    Merge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearnedSkillBucket {
    Planning,
    Verification,
    Review,
    Execution,
    Merge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearnedSkillRecord {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub status: LearnedSkillStatus,
    #[serde(default)]
    pub pinned: bool,
    pub caller_surfaces: Vec<String>,
    pub stages: Vec<LearnedSkillStage>,
    pub buckets: Vec<LearnedSkillBucket>,
    pub path_scopes: Vec<String>,
    pub compact_guidance: String,
    pub predicted_effect: String,
    pub provenance_refs: Vec<String>,
}

impl LearnedSkillRecord {
    pub fn with_pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

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
pub struct LearnedSkillMultiSelectionRequest {
    pub project_id: String,
    pub caller_surface: String,
    pub stages: Vec<LearnedSkillStage>,
    pub buckets: Vec<LearnedSkillBucket>,
    pub touched_paths: Vec<String>,
    pub max_skills: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectSkillMappingError {
    EmptyId,
    EmptyProjectId,
    EmptyTitle,
    EmptyGuidance,
    EmptyCallerSurface,
    InvalidBucket(String),
    InvalidStage(String),
    InvalidPipelineRole,
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
        PlanModeVerdict::Accepted => "accepted",
        PlanModeVerdict::Declined => "declined",
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
    select_pre_execution_learned_skills_multi(
        LearnedSkillMultiSelectionRequest {
            project_id: request.project_id,
            caller_surface: request.caller_surface,
            stages: vec![request.stage],
            buckets: vec![request.bucket],
            touched_paths: request.touched_paths,
            max_skills: request.max_skills,
        },
        skills,
    )
}

pub fn select_pre_execution_learned_skills_multi(
    request: LearnedSkillMultiSelectionRequest,
    skills: &[LearnedSkillRecord],
) -> Vec<LearnedSkillRecord> {
    if request.max_skills == 0 || request.stages.is_empty() || request.buckets.is_empty() {
        return Vec::new();
    }

    let mut selected = skills
        .iter()
        .filter(|skill| skill.status == LearnedSkillStatus::Approved)
        .filter(|skill| skill.project_id == request.project_id)
        .filter(|skill| caller_surface_matches(&request.caller_surface, &skill.caller_surfaces))
        .filter(|skill| {
            request
                .stages
                .iter()
                .any(|stage| skill.stages.contains(stage))
        })
        .filter(|skill| {
            request
                .buckets
                .iter()
                .any(|bucket| skill.buckets.contains(bucket))
        })
        .filter(|skill| path_scope_matches(&request.touched_paths, &skill.path_scopes))
        .cloned()
        .collect::<Vec<_>>();
    selected.sort_by(|left, right| {
        right
            .pinned
            .cmp(&left.pinned)
            .then_with(|| {
                bucket_rank(left, &request.buckets).cmp(&bucket_rank(right, &request.buckets))
            })
            .then_with(|| left.id.cmp(&right.id))
            .then_with(|| left.title.cmp(&right.title))
    });
    let mut seen = HashSet::new();
    selected.retain(|skill| seen.insert(skill.id.clone()));
    selected.truncate(request.max_skills);
    selected
}

fn bucket_rank(skill: &LearnedSkillRecord, buckets: &[LearnedSkillBucket]) -> usize {
    buckets
        .iter()
        .position(|bucket| skill.buckets.contains(bucket))
        .unwrap_or(usize::MAX)
}

pub fn project_skill_to_learned_skill_record(
    skill: &ProjectSkill,
    caller_surface: &str,
) -> Result<LearnedSkillRecord, ProjectSkillMappingError> {
    let id = nonempty(skill.id.as_str()).ok_or(ProjectSkillMappingError::EmptyId)?;
    let project_id =
        nonempty(skill.project_id.as_str()).ok_or(ProjectSkillMappingError::EmptyProjectId)?;
    let title = nonempty(&skill.title).ok_or(ProjectSkillMappingError::EmptyTitle)?;
    let compact_guidance =
        nonempty(&skill.compact_guidance).ok_or(ProjectSkillMappingError::EmptyGuidance)?;
    let caller_surface =
        nonempty(caller_surface).ok_or(ProjectSkillMappingError::EmptyCallerSurface)?;
    let bucket = parse_bucket(&skill.bucket)?;
    let stage = parse_stage(&skill.stage)?;
    if skill
        .pipeline_role
        .as_deref()
        .is_some_and(|role| role.trim().is_empty())
    {
        return Err(ProjectSkillMappingError::InvalidPipelineRole);
    }

    Ok(LearnedSkillRecord {
        id: id.to_string(),
        project_id: project_id.to_string(),
        title: title.to_string(),
        status: learned_status(skill.status),
        pinned: skill.pinned,
        caller_surfaces: vec![caller_surface.to_string()],
        stages: vec![stage],
        buckets: vec![bucket],
        path_scopes: skill.scope_paths.clone(),
        compact_guidance: compact_guidance.to_string(),
        predicted_effect: skill
            .predicted_effect
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string(),
        provenance_refs: project_skill_provenance_refs(skill),
    })
}

fn nonempty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn parse_bucket(value: &str) -> Result<LearnedSkillBucket, ProjectSkillMappingError> {
    match value.trim() {
        "planning" => Ok(LearnedSkillBucket::Planning),
        "verification" => Ok(LearnedSkillBucket::Verification),
        "review" => Ok(LearnedSkillBucket::Review),
        "execution" => Ok(LearnedSkillBucket::Execution),
        "merge" => Ok(LearnedSkillBucket::Merge),
        other => Err(ProjectSkillMappingError::InvalidBucket(other.to_string())),
    }
}

fn parse_stage(value: &str) -> Result<LearnedSkillStage, ProjectSkillMappingError> {
    match value.trim() {
        "planning" => Ok(LearnedSkillStage::Planning),
        "verification" => Ok(LearnedSkillStage::Verification),
        "review" => Ok(LearnedSkillStage::Review),
        "execution" => Ok(LearnedSkillStage::Execution),
        "merge" => Ok(LearnedSkillStage::Merge),
        other => Err(ProjectSkillMappingError::InvalidStage(other.to_string())),
    }
}

fn learned_status(status: ProjectSkillLifecycleStatus) -> LearnedSkillStatus {
    match status {
        ProjectSkillLifecycleStatus::Staged => LearnedSkillStatus::Staged,
        ProjectSkillLifecycleStatus::Approved => LearnedSkillStatus::Approved,
        ProjectSkillLifecycleStatus::Rejected => LearnedSkillStatus::Rejected,
        ProjectSkillLifecycleStatus::Stale => LearnedSkillStatus::Stale,
        ProjectSkillLifecycleStatus::Archived => LearnedSkillStatus::Archived,
        ProjectSkillLifecycleStatus::Retired => LearnedSkillStatus::Retired,
    }
}

fn project_skill_provenance_refs(skill: &ProjectSkill) -> Vec<String> {
    let mut refs = Vec::new();
    collect_provenance_refs(&skill.provenance_json, None, &mut refs);
    refs.sort();
    refs.dedup();
    refs
}

fn collect_provenance_refs(value: &serde_json::Value, key: Option<&str>, refs: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                collect_provenance_refs(value, Some(key), refs);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                collect_provenance_refs(value, key, refs);
            }
        }
        serde_json::Value::String(value)
            if key.is_some_and(|key| {
                key == "id"
                    || key.ends_with("_id")
                    || key.ends_with("_ids")
                    || key.ends_with("_ref")
                    || key.ends_with("_refs")
            }) =>
        {
            if let Some(value) = nonempty(value) {
                refs.push(value.to_string());
            }
        }
        _ => {}
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::ideation::{VerificationGap, VerificationRoundSnapshot};

    fn skill(id: &str, project_id: &str, status: LearnedSkillStatus) -> LearnedSkillRecord {
        LearnedSkillRecord {
            id: id.to_string(),
            project_id: project_id.to_string(),
            title: format!("Skill {id}"),
            status,
            pinned: false,
            caller_surfaces: Vec::new(),
            stages: Vec::new(),
            buckets: Vec::new(),
            path_scopes: Vec::new(),
            compact_guidance: "Keep the learned guidance compact.".to_string(),
            predicted_effect: "Reduce repeated mistakes.".to_string(),
            provenance_refs: Vec::new(),
        }
    }

    fn round(round: u32, descriptions: &[&str]) -> VerificationRoundSnapshot {
        let gaps = descriptions
            .iter()
            .map(|description| VerificationGap {
                severity: "high".to_string(),
                category: "testing".to_string(),
                description: (*description).to_string(),
                why_it_matters: Some(
                    "Recurring verification gaps should stay descriptive until gated.".to_string(),
                ),
                source: Some("layer2".to_string()),
            })
            .collect::<Vec<_>>();
        VerificationRoundSnapshot {
            round,
            fingerprints: gaps
                .iter()
                .map(|gap| gap_fingerprint(&gap.description))
                .collect(),
            gap_score: gaps.len() as u32 * 3,
            gaps,
            parse_failed: false,
        }
    }

    #[test]
    fn plan_mode_capture_records_accept_decline_and_suppression_paths() {
        let accepted = capture_plan_mode_verdict(PlanModeVerdictCaptureInput {
            project_id: "project-1".to_string(),
            conversation_id: "conversation-1".to_string(),
            planning_session_id: Some("planning-session-1".to_string()),
            accepted_session_id: Some("accepted-session-1".to_string()),
            plan_artifact_id: Some("plan-1".to_string()),
            verdict: PlanModeVerdict::Accepted,
            reason: Some("Architecture work needs plan mode.".to_string()),
        })
        .expect("accepted verdict should be captured");

        assert_eq!(accepted.outcome_class, "accepted");
        assert_eq!(accepted.status, "eligible");
        assert_eq!(
            accepted.refs.get("accepted_session_id").map(String::as_str),
            Some("accepted-session-1")
        );
        assert_eq!(
            accepted.refs.get("plan_artifact_id").map(String::as_str),
            Some("plan-1")
        );
        assert!(!accepted.mutates_accepted_session);
        assert!(accepted.evidence_summary.contains("Architecture work"));

        let declined = capture_plan_mode_verdict(PlanModeVerdictCaptureInput {
            project_id: "project-1".to_string(),
            conversation_id: "conversation-1".to_string(),
            planning_session_id: Some("planning-session-1".to_string()),
            accepted_session_id: None,
            plan_artifact_id: None,
            verdict: PlanModeVerdict::Declined,
            reason: Some(" ".to_string()),
        })
        .expect("declined verdict should be captured");
        assert_eq!(declined.outcome_class, "declined");
        assert_eq!(
            declined.evidence_summary,
            "Plan-mode verdict captured without model transcript body."
        );

        assert!(capture_plan_mode_verdict(PlanModeVerdictCaptureInput {
            project_id: "project-1".to_string(),
            conversation_id: "conversation-1".to_string(),
            planning_session_id: Some("planning-session-1".to_string()),
            accepted_session_id: None,
            plan_artifact_id: None,
            verdict: PlanModeVerdict::Skipped,
            reason: Some("Narrow edit.".to_string()),
        })
        .is_none());
        assert!(capture_plan_mode_verdict(PlanModeVerdictCaptureInput {
            project_id: "project-1".to_string(),
            conversation_id: "conversation-1".to_string(),
            planning_session_id: None,
            accepted_session_id: Some("accepted-session-1".to_string()),
            plan_artifact_id: None,
            verdict: PlanModeVerdict::Accepted,
            reason: None,
        })
        .is_none());
    }

    #[test]
    fn plan_mode_capture_truncates_long_reasons() {
        let outcome = capture_plan_mode_verdict(PlanModeVerdictCaptureInput {
            project_id: "project-1".to_string(),
            conversation_id: "conversation-1".to_string(),
            planning_session_id: Some("planning-session-1".to_string()),
            accepted_session_id: None,
            plan_artifact_id: None,
            verdict: PlanModeVerdict::Accepted,
            reason: Some("x".repeat(300)),
        })
        .expect("accepted verdict should be captured");

        assert_eq!(outcome.evidence_summary.chars().count(), 243);
        assert!(outcome.evidence_summary.ends_with("..."));
    }

    #[test]
    fn pre_execution_selection_filters_sorts_and_limits_skills() {
        let selected = select_pre_execution_learned_skills(
            LearnedSkillSelectionRequest {
                project_id: "project-1".to_string(),
                caller_surface: "ralphx-execution-worker".to_string(),
                stage: LearnedSkillStage::Execution,
                bucket: LearnedSkillBucket::Execution,
                touched_paths: vec!["src-tauri/src/application/chat_service/mod.rs".to_string()],
                max_skills: 1,
            },
            &[
                skill("skill-z", "project-1", LearnedSkillStatus::Approved)
                    .with_caller_surfaces(vec!["ralphx-execution-worker"])
                    .with_stages(vec![LearnedSkillStage::Execution])
                    .with_buckets(vec![LearnedSkillBucket::Execution])
                    .with_path_scopes(vec!["src-tauri/src/application"])
                    .with_predicted_effect("Reduce repeated execution misses.")
                    .with_provenance_refs(vec!["outcome-1"]),
                skill("skill-a", "project-1", LearnedSkillStatus::Approved)
                    .with_caller_surfaces(vec!["ralphx-execution-worker"])
                    .with_stages(vec![LearnedSkillStage::Execution])
                    .with_buckets(vec![LearnedSkillBucket::Execution])
                    .with_path_scopes(vec!["src-tauri/src/application"]),
                skill("skill-surface", "project-1", LearnedSkillStatus::Approved)
                    .with_caller_surfaces(vec!["ralphx-review-history"])
                    .with_stages(vec![LearnedSkillStage::Execution])
                    .with_buckets(vec![LearnedSkillBucket::Execution])
                    .with_path_scopes(vec!["src-tauri/src/application"]),
                skill("skill-staged", "project-1", LearnedSkillStatus::Staged)
                    .with_caller_surfaces(vec!["ralphx-execution-worker"])
                    .with_stages(vec![LearnedSkillStage::Execution])
                    .with_buckets(vec![LearnedSkillBucket::Execution])
                    .with_path_scopes(vec!["src-tauri/src/application"]),
                skill("skill-project", "project-2", LearnedSkillStatus::Approved)
                    .with_caller_surfaces(vec!["ralphx-execution-worker"])
                    .with_stages(vec![LearnedSkillStage::Execution])
                    .with_buckets(vec![LearnedSkillBucket::Execution])
                    .with_path_scopes(vec!["src-tauri/src/application"]),
                skill("skill-stage", "project-1", LearnedSkillStatus::Approved)
                    .with_caller_surfaces(vec!["ralphx-execution-worker"])
                    .with_stages(vec![LearnedSkillStage::Planning])
                    .with_buckets(vec![LearnedSkillBucket::Execution])
                    .with_path_scopes(vec!["src-tauri/src/application"]),
                skill("skill-bucket", "project-1", LearnedSkillStatus::Approved)
                    .with_caller_surfaces(vec!["ralphx-execution-worker"])
                    .with_stages(vec![LearnedSkillStage::Execution])
                    .with_buckets(vec![LearnedSkillBucket::Review])
                    .with_path_scopes(vec!["src-tauri/src/application"]),
                skill("skill-path", "project-1", LearnedSkillStatus::Approved)
                    .with_caller_surfaces(vec!["ralphx-execution-worker"])
                    .with_stages(vec![LearnedSkillStage::Execution])
                    .with_buckets(vec![LearnedSkillBucket::Execution])
                    .with_path_scopes(vec!["frontend/src"]),
            ],
        );

        assert_eq!(
            selected
                .iter()
                .map(|skill| skill.id.as_str())
                .collect::<Vec<_>>(),
            vec!["skill-a"]
        );
    }

    #[test]
    fn pre_execution_selection_rejects_invalid_or_empty_scope_inputs() {
        for touched_paths in [
            vec!["/src-tauri/src/application/chat_service/mod.rs".to_string()],
            vec!["src-tauri\\src\\application".to_string()],
            vec!["src-tauri//src".to_string()],
            Vec::new(),
        ] {
            let selected = select_pre_execution_learned_skills(
                LearnedSkillSelectionRequest {
                    project_id: "project-1".to_string(),
                    caller_surface: "ralphx-execution-worker".to_string(),
                    stage: LearnedSkillStage::Execution,
                    bucket: LearnedSkillBucket::Execution,
                    touched_paths,
                    max_skills: 4,
                },
                &[
                    skill("skill-match", "project-1", LearnedSkillStatus::Approved)
                        .with_caller_surfaces(vec!["ralphx-execution-worker"])
                        .with_stages(vec![LearnedSkillStage::Execution])
                        .with_buckets(vec![LearnedSkillBucket::Execution])
                        .with_path_scopes(vec!["src-tauri/src/application"]),
                ],
            );
            assert!(selected.is_empty());
        }
    }

    #[test]
    fn pre_execution_selection_handles_empty_limits_and_unscoped_skills() {
        let approved = skill("skill-match", "project-1", LearnedSkillStatus::Approved)
            .with_caller_surfaces(vec!["ralphx-execution-worker"])
            .with_stages(vec![LearnedSkillStage::Execution])
            .with_buckets(vec![LearnedSkillBucket::Execution]);

        assert!(select_pre_execution_learned_skills(
            LearnedSkillSelectionRequest {
                project_id: "project-1".to_string(),
                caller_surface: "ralphx-execution-worker".to_string(),
                stage: LearnedSkillStage::Execution,
                bucket: LearnedSkillBucket::Execution,
                touched_paths: vec!["src-tauri/src/application/mod.rs".to_string()],
                max_skills: 0,
            },
            std::slice::from_ref(&approved),
        )
        .is_empty());

        let selected = select_pre_execution_learned_skills(
            LearnedSkillSelectionRequest {
                project_id: "project-1".to_string(),
                caller_surface: " ralphx-execution-worker ".to_string(),
                stage: LearnedSkillStage::Execution,
                bucket: LearnedSkillBucket::Execution,
                touched_paths: Vec::new(),
                max_skills: 4,
            },
            &[approved],
        );
        assert_eq!(selected.len(), 1);
    }

    #[test]
    fn constraint_bundle_citations_copy_relevant_fields() {
        let selected = vec![skill(
            "skill-review-loop",
            "project-1",
            LearnedSkillStatus::Approved,
        )
        .with_predicted_effect("Reduce repeated verification gaps.")
        .with_provenance_refs(vec!["outcome-1", "verification-round-3"])];

        let citations = build_constraint_bundle_skill_citations(&selected);

        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].skill_id, "skill-review-loop");
        assert_eq!(
            citations[0].predicted_effect,
            "Reduce repeated verification gaps."
        );
        assert_eq!(
            citations[0].provenance_refs,
            vec!["outcome-1".to_string(), "verification-round-3".to_string()]
        );
    }

    #[test]
    fn verification_gap_recurrence_reports_all_gate_outcomes() {
        let below_corpus = verification_gap_recurrence_candidates(
            &[round(1, &["Missing reviewer regression test"])],
            VerificationGapRecurrenceGate {
                min_occurrences: 2,
                min_rounds: 2,
                min_corpus_size: 3,
            },
        );
        assert_eq!(below_corpus.corpus_size, 1);
        assert_eq!(
            below_corpus.suppressed_gaps[0].reason,
            VerificationGapSuppressionReason::BelowCorpusGate
        );

        let below_occurrence = verification_gap_recurrence_candidates(
            &[
                round(1, &["Missing reviewer regression test"]),
                round(2, &["Document export preview"]),
                round(3, &["Trace native skill imports"]),
            ],
            VerificationGapRecurrenceGate {
                min_occurrences: 2,
                min_rounds: 2,
                min_corpus_size: 3,
            },
        );
        assert!(below_occurrence.recurring_gaps.is_empty());
        assert!(below_occurrence
            .suppressed_gaps
            .iter()
            .all(|gap| gap.reason == VerificationGapSuppressionReason::BelowOccurrenceGate));

        let below_round = verification_gap_recurrence_candidates(
            &[round(
                1,
                &[
                    "Missing reviewer regression test",
                    "Missing reviewer regression test",
                    "Document export preview",
                ],
            )],
            VerificationGapRecurrenceGate {
                min_occurrences: 2,
                min_rounds: 2,
                min_corpus_size: 3,
            },
        );
        assert!(below_round
            .suppressed_gaps
            .iter()
            .any(|gap| gap.reason == VerificationGapSuppressionReason::BelowRoundGate));

        let promoted = verification_gap_recurrence_candidates(
            &[
                round(1, &["Missing reviewer regression test"]),
                round(2, &["reviewer regression test missing"]),
                round(3, &["Missing reviewer regression test"]),
            ],
            VerificationGapRecurrenceGate {
                min_occurrences: 2,
                min_rounds: 2,
                min_corpus_size: 3,
            },
        );
        assert_eq!(promoted.recurring_gaps.len(), 1);
        assert_eq!(promoted.recurring_gaps[0].occurrences, 3);
        assert_eq!(promoted.recurring_gaps[0].distinct_rounds, 3);
        assert!(promoted.recurring_gaps[0]
            .descriptions
            .contains(&"Missing reviewer regression test".to_string()));
    }
}
