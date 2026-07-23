use std::sync::Arc;

use chrono::Utc;
use serde_json::{Map, Value};

use crate::domain::entities::{
    prepare_new_project_skill, refresh_project_skill_metadata,
    validate_project_skill_pipeline_role, ProjectSkill, ProjectSkillId,
    ProjectSkillLifecycleStatus,
};
use crate::domain::repositories::{
    ProjectSkillMatchedMutation, ProjectSkillRepository, ProjectSkillResolutionCommand,
    ProjectSkillResolutionIdentity, ProjectSkillResolutionIdentityKind,
    ProjectSkillResolutionIntent, ProjectSkillResolutionOutcome, ProjectSkillResolutionResult,
    ProjectSkillStagingPolicy,
};
use crate::error::{AppError, AppResult};

pub struct ProjectSkillResolutionService {
    repo: Arc<dyn ProjectSkillRepository>,
}

pub const PROJECT_SKILL_STAGING_CAP_ERROR: &str =
    "project skill staging limit reached for this project, bucket, and pipeline role";

impl ProjectSkillResolutionService {
    pub fn new(repo: Arc<dyn ProjectSkillRepository>) -> Self {
        Self { repo }
    }

    pub async fn resolve(
        &self,
        command: ProjectSkillResolutionCommand,
    ) -> AppResult<ProjectSkillResolutionResult> {
        validate_resolution_command(&command)?;
        self.repo.resolve(command).await
    }
}

#[derive(Debug)]
pub(crate) struct ProjectSkillResolutionPlan {
    pub outcome: ProjectSkillResolutionOutcome,
    pub skill: ProjectSkill,
}

pub(crate) fn evaluate_project_skill_resolution(
    mut command: ProjectSkillResolutionCommand,
    candidates: &[ProjectSkill],
) -> AppResult<ProjectSkillResolutionPlan> {
    command.candidate = prepare_new_project_skill(command.candidate);
    validate_resolution_command(&command)?;
    if candidates
        .iter()
        .any(|skill| skill.project_id != command.candidate.project_id)
    {
        return Err(AppError::Validation(
            "project skill resolution candidates must be project-scoped".to_string(),
        ));
    }

    match &command.intent {
        ProjectSkillResolutionIntent::ExplicitPatch { target_id } => {
            resolve_explicit_patch(&command, candidates, target_id)
        }
        ProjectSkillResolutionIntent::SourceSync { identity } => {
            resolve_source_sync(&command, candidates, identity)
        }
        ProjectSkillResolutionIntent::Upsert {
            identities,
            matched_mutation,
        } => resolve_upsert(&command, candidates, identities, *matched_mutation),
    }
}

pub(crate) fn enforce_project_skill_staging_policy(
    policy: Option<&ProjectSkillStagingPolicy>,
    candidates: &[ProjectSkill],
    plan: &ProjectSkillResolutionPlan,
) -> AppResult<()> {
    let Some(policy) = policy else {
        return Ok(());
    };
    validate_staging_policy(policy)?;
    if plan.outcome != ProjectSkillResolutionOutcome::CreateNew {
        return Ok(());
    }
    if plan.skill.pipeline_role.as_deref() != Some(policy.pipeline_role.as_str()) {
        return Err(AppError::Validation(
            "project skill staging policy role must match trusted candidate attribution"
                .to_string(),
        ));
    }

    let mut staged_count = 0usize;
    for candidate in candidates.iter().filter(|candidate| {
        candidate.status == ProjectSkillLifecycleStatus::Staged
            && candidate.bucket == plan.skill.bucket
            && candidate.created_at >= policy.window_start
    }) {
        validate_project_skill_pipeline_role(candidate.pipeline_role.as_deref()).map_err(
            |error| {
                AppError::Database(format!(
                    "invalid persisted project skill pipeline role: {error}"
                ))
            },
        )?;
        if candidate.pipeline_role.is_none()
            || candidate.pipeline_role.as_deref() == Some(policy.pipeline_role.as_str())
        {
            staged_count = staged_count.checked_add(1).ok_or_else(|| {
                AppError::Conflict("project skill staging count overflow".to_string())
            })?;
        }
    }
    if staged_count >= policy.max_staged {
        return Err(AppError::Conflict(
            PROJECT_SKILL_STAGING_CAP_ERROR.to_string(),
        ));
    }
    Ok(())
}

pub fn project_skill_resolution_identities(
    skill: &ProjectSkill,
) -> Vec<ProjectSkillResolutionIdentity> {
    let mut identities = Vec::new();
    push_provenance_identity(
        &mut identities,
        skill,
        ProjectSkillResolutionIdentityKind::Outcome,
        &["outcome_id"],
    );
    push_provenance_identity(
        &mut identities,
        skill,
        ProjectSkillResolutionIdentityKind::Recurrence,
        &["additional", "recurrence_key"],
    );
    push_provenance_identity(
        &mut identities,
        skill,
        ProjectSkillResolutionIdentityKind::VerificationGap,
        &["additional", "verification_gap_fingerprint"],
    );
    push_provenance_identity(
        &mut identities,
        skill,
        ProjectSkillResolutionIdentityKind::PullRequest,
        &["source_ref_id"],
    );
    if let Some(number) = skill
        .provenance_json
        .get("pull_request_number")
        .and_then(Value::as_i64)
    {
        identities.push(ProjectSkillResolutionIdentity {
            kind: ProjectSkillResolutionIdentityKind::PullRequest,
            value: number.to_string(),
        });
    }
    push_provenance_identity(
        &mut identities,
        skill,
        ProjectSkillResolutionIdentityKind::ImportExternal,
        &["external_id"],
    );
    push_provenance_identity(
        &mut identities,
        skill,
        ProjectSkillResolutionIdentityKind::Memory,
        &["memory_id"],
    );
    if identities.is_empty() {
        identities.push(ProjectSkillResolutionIdentity {
            kind: ProjectSkillResolutionIdentityKind::Content,
            value: skill.content_hash.clone(),
        });
    }
    identities.sort_by(|left, right| {
        identity_kind_rank(left.kind)
            .cmp(&identity_kind_rank(right.kind))
            .then_with(|| left.value.cmp(&right.value))
    });
    identities.dedup();
    identities
}

pub fn import_title_resolution_identity(
    title: &str,
    bucket: &str,
    stage: &str,
) -> ProjectSkillResolutionIdentity {
    ProjectSkillResolutionIdentity {
        kind: ProjectSkillResolutionIdentityKind::ImportTitle,
        value: normalized_import_title(title, bucket, stage),
    }
}

fn resolve_explicit_patch(
    command: &ProjectSkillResolutionCommand,
    candidates: &[ProjectSkill],
    target_id: &ProjectSkillId,
) -> AppResult<ProjectSkillResolutionPlan> {
    let target = candidates
        .iter()
        .find(|skill| skill.id == *target_id)
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "project skill {} was not found",
                target_id.as_str()
            ))
        })?;
    validate_explicit_target(target)?;
    if target.status == ProjectSkillLifecycleStatus::Approved {
        if resolution_is_duplicate(command, target, ProjectSkillMatchedMutation::PatchExisting) {
            return Ok(duplicate_plan(target));
        }
        return resolve_approved_revision(command, candidates, target);
    }
    resolve_existing(command, target, ProjectSkillMatchedMutation::PatchExisting)
}

fn resolve_source_sync(
    command: &ProjectSkillResolutionCommand,
    candidates: &[ProjectSkill],
    identity: &ProjectSkillResolutionIdentity,
) -> AppResult<ProjectSkillResolutionPlan> {
    let matches = matching_candidates(candidates, std::slice::from_ref(identity));
    let Some(target) = select_preferred_match(matches)? else {
        return Err(AppError::NotFound(
            "source-tracked project skill was not found".to_string(),
        ));
    };
    if !project_skill_source_sync_enabled(target) {
        return Ok(duplicate_plan(target));
    }
    if target.status == ProjectSkillLifecycleStatus::Approved {
        if resolution_is_duplicate(command, target, ProjectSkillMatchedMutation::PatchExisting) {
            return Ok(duplicate_plan(target));
        }
        return resolve_approved_revision(command, candidates, target);
    }
    resolve_existing(command, target, ProjectSkillMatchedMutation::PatchExisting)
}

fn resolve_upsert(
    command: &ProjectSkillResolutionCommand,
    candidates: &[ProjectSkill],
    identities: &[ProjectSkillResolutionIdentity],
    matched_mutation: ProjectSkillMatchedMutation,
) -> AppResult<ProjectSkillResolutionPlan> {
    let matches = matching_candidates(candidates, identities);
    let Some(target) = select_preferred_match(matches)? else {
        return create_plan(command, None);
    };
    if identities.iter().any(|identity| {
        identity.kind == ProjectSkillResolutionIdentityKind::Outcome
            && skill_matches_identity(target, identity)
    }) {
        return Ok(duplicate_plan(target));
    }
    if target.status == ProjectSkillLifecycleStatus::Approved {
        if resolution_is_duplicate(command, target, matched_mutation) {
            return Ok(duplicate_plan(target));
        }
        return resolve_approved_revision(command, candidates, target);
    }
    resolve_existing(command, target, matched_mutation)
}

fn resolve_approved_revision(
    command: &ProjectSkillResolutionCommand,
    candidates: &[ProjectSkill],
    approved: &ProjectSkill,
) -> AppResult<ProjectSkillResolutionPlan> {
    let companions = candidates
        .iter()
        .filter(|skill| is_eligible_match(skill))
        .filter(|skill| {
            skill.status == ProjectSkillLifecycleStatus::Staged
                && skill.companion_of_skill_id.as_ref() == Some(&approved.id)
        })
        .collect::<Vec<_>>();
    match companions.as_slice() {
        [] => create_plan(command, Some(approved)),
        [companion] => resolve_existing(
            command,
            companion,
            match command.intent {
                ProjectSkillResolutionIntent::Upsert {
                    matched_mutation, ..
                } => matched_mutation,
                ProjectSkillResolutionIntent::ExplicitPatch { .. }
                | ProjectSkillResolutionIntent::SourceSync { .. } => {
                    ProjectSkillMatchedMutation::PatchExisting
                }
            },
        ),
        _ => Err(AppError::Conflict(format!(
            "approved project skill {} has multiple active pending revisions",
            approved.id.as_str()
        ))),
    }
}

fn resolve_existing(
    command: &ProjectSkillResolutionCommand,
    current: &ProjectSkill,
    matched_mutation: ProjectSkillMatchedMutation,
) -> AppResult<ProjectSkillResolutionPlan> {
    if resolution_is_duplicate(command, current, matched_mutation) {
        return Ok(duplicate_plan(current));
    }

    let mut skill = current.clone();
    let outcome = match matched_mutation {
        ProjectSkillMatchedMutation::PatchExisting => {
            copy_candidate_content(&mut skill, &command.candidate);
            merge_json(
                &mut skill.provenance_json,
                &command.candidate.provenance_json,
            );
            ProjectSkillResolutionOutcome::PatchExisting
        }
        ProjectSkillMatchedMutation::AppendEvidence => {
            append_resolution_evidence(
                &mut skill.provenance_json,
                &command.candidate.provenance_json,
            );
            if let Some(appendix) = command
                .evidence_markdown
                .as_deref()
                .map(str::trim)
                .filter(|appendix| !appendix.is_empty())
            {
                if !skill.body_markdown.contains(appendix) {
                    skill.body_markdown.push_str("\n\n");
                    skill.body_markdown.push_str(appendix);
                }
            }
            ProjectSkillResolutionOutcome::AppendEvidence
        }
    };
    skill.updated_at = Utc::now();
    refresh_project_skill_metadata(&mut skill);
    Ok(ProjectSkillResolutionPlan { outcome, skill })
}

fn resolution_is_duplicate(
    command: &ProjectSkillResolutionCommand,
    current: &ProjectSkill,
    matched_mutation: ProjectSkillMatchedMutation,
) -> bool {
    let incoming_evidence_present =
        provenance_contains(&current.provenance_json, &command.candidate.provenance_json);
    match matched_mutation {
        ProjectSkillMatchedMutation::PatchExisting => {
            project_skill_payload_matches(current, &command.candidate) && incoming_evidence_present
        }
        ProjectSkillMatchedMutation::AppendEvidence => {
            let appendix_present = command.evidence_markdown.as_ref().map_or(true, |appendix| {
                appendix.trim().is_empty() || current.body_markdown.contains(appendix.trim())
            });
            incoming_evidence_present && appendix_present
        }
    }
}

fn create_plan(
    command: &ProjectSkillResolutionCommand,
    approved: Option<&ProjectSkill>,
) -> AppResult<ProjectSkillResolutionPlan> {
    let mut skill = command.candidate.clone();
    skill.status = ProjectSkillLifecycleStatus::Staged;
    skill.pinned = false;
    skill.archived = false;
    if let Some(approved) = approved {
        if approved.project_id != skill.project_id {
            return Err(AppError::Validation(
                "pending project skill revision belongs to a different project".to_string(),
            ));
        }
        let mut provenance = approved.provenance_json.clone();
        merge_json(&mut provenance, &skill.provenance_json);
        if let Value::Object(object) = &mut provenance {
            object.insert(
                "pending_revision_of".to_string(),
                Value::String(approved.id.as_str().to_string()),
            );
        }
        skill.provenance_json = provenance;
        skill.companion_of_skill_id = Some(approved.id.clone());
    }
    skill.updated_at = Utc::now();
    refresh_project_skill_metadata(&mut skill);
    Ok(ProjectSkillResolutionPlan {
        outcome: ProjectSkillResolutionOutcome::CreateNew,
        skill,
    })
}

fn duplicate_plan(skill: &ProjectSkill) -> ProjectSkillResolutionPlan {
    ProjectSkillResolutionPlan {
        outcome: ProjectSkillResolutionOutcome::Duplicate,
        skill: skill.clone(),
    }
}

fn matching_candidates<'a>(
    candidates: &'a [ProjectSkill],
    identities: &[ProjectSkillResolutionIdentity],
) -> Vec<&'a ProjectSkill> {
    candidates
        .iter()
        .filter(|skill| is_eligible_match(skill))
        .filter(|skill| {
            identities
                .iter()
                .any(|identity| skill_matches_identity(skill, identity))
        })
        .collect()
}

fn select_preferred_match(matches: Vec<&ProjectSkill>) -> AppResult<Option<&ProjectSkill>> {
    let Some(best_rank) = matches.iter().map(|skill| lifecycle_rank(skill)).min() else {
        return Ok(None);
    };
    let preferred = matches
        .into_iter()
        .filter(|skill| lifecycle_rank(skill) == best_rank)
        .collect::<Vec<_>>();
    match preferred.as_slice() {
        [skill] => Ok(Some(*skill)),
        _ => Err(AppError::Conflict(
            "project skill resolution matched multiple equally preferred active skills".to_string(),
        )),
    }
}

fn lifecycle_rank(skill: &ProjectSkill) -> u8 {
    match skill.status {
        ProjectSkillLifecycleStatus::Staged if skill.companion_of_skill_id.is_some() => 0,
        ProjectSkillLifecycleStatus::Staged => 1,
        ProjectSkillLifecycleStatus::Stale => 2,
        ProjectSkillLifecycleStatus::Approved => 3,
        ProjectSkillLifecycleStatus::Rejected
        | ProjectSkillLifecycleStatus::Archived
        | ProjectSkillLifecycleStatus::Retired => 4,
    }
}

fn is_eligible_match(skill: &ProjectSkill) -> bool {
    !skill.archived
        && matches!(
            skill.status,
            ProjectSkillLifecycleStatus::Staged
                | ProjectSkillLifecycleStatus::Approved
                | ProjectSkillLifecycleStatus::Stale
        )
}

fn validate_explicit_target(skill: &ProjectSkill) -> AppResult<()> {
    if !is_eligible_match(skill) {
        return Err(AppError::Validation(
            "rejected, archived, or retired project skills cannot be edited".to_string(),
        ));
    }
    Ok(())
}

fn validate_resolution_command(command: &ProjectSkillResolutionCommand) -> AppResult<()> {
    if command.candidate.title.trim().is_empty()
        || command.candidate.bucket.trim().is_empty()
        || command.candidate.stage.trim().is_empty()
        || command.candidate.compact_guidance.trim().is_empty()
        || command.candidate.body_markdown.trim().is_empty()
    {
        return Err(AppError::Validation(
            "project skill resolution candidate is incomplete".to_string(),
        ));
    }
    if let Some(policy) = command.staging_policy.as_ref() {
        validate_staging_policy(policy)?;
    }
    match &command.intent {
        ProjectSkillResolutionIntent::Upsert { identities, .. } if identities.is_empty() => {
            Err(AppError::Validation(
                "project skill resolution requires at least one identity".to_string(),
            ))
        }
        ProjectSkillResolutionIntent::Upsert { identities, .. } => {
            for identity in identities {
                validate_identity(identity)?;
            }
            Ok(())
        }
        ProjectSkillResolutionIntent::SourceSync { identity } => validate_identity(identity),
        ProjectSkillResolutionIntent::ExplicitPatch { target_id }
            if target_id.as_str().trim().is_empty() =>
        {
            Err(AppError::Validation(
                "project skill explicit target is required".to_string(),
            ))
        }
        ProjectSkillResolutionIntent::ExplicitPatch { .. } => Ok(()),
    }
}

fn validate_staging_policy(policy: &ProjectSkillStagingPolicy) -> AppResult<()> {
    if policy.pipeline_role.trim().is_empty() || policy.pipeline_role != policy.pipeline_role.trim()
    {
        return Err(AppError::Validation(
            "project skill staging policy requires a trimmed pipeline role".to_string(),
        ));
    }
    if policy.max_staged == 0 {
        return Err(AppError::Validation(
            "project skill staging policy limit must be positive".to_string(),
        ));
    }
    Ok(())
}

fn validate_identity(identity: &ProjectSkillResolutionIdentity) -> AppResult<()> {
    if normalized_identity(&identity.value).is_empty() {
        return Err(AppError::Validation(
            "project skill resolution identity is required".to_string(),
        ));
    }
    Ok(())
}

fn skill_matches_identity(skill: &ProjectSkill, identity: &ProjectSkillResolutionIdentity) -> bool {
    let expected = normalized_identity(&identity.value);
    skill_identity_values(skill, identity.kind)
        .into_iter()
        .any(|value| normalized_identity(&value) == expected)
}

fn skill_identity_values(
    skill: &ProjectSkill,
    kind: ProjectSkillResolutionIdentityKind,
) -> Vec<String> {
    match kind {
        ProjectSkillResolutionIdentityKind::Content => vec![skill.content_hash.clone()],
        ProjectSkillResolutionIdentityKind::Outcome => {
            let mut values = provenance_string_values(&skill.provenance_json, &["outcome_id"]);
            values.extend(provenance_array_string_values(
                &skill.provenance_json,
                &["outcome_ids"],
            ));
            values.extend(provenance_array_string_values(
                &skill.provenance_json,
                &["evidence_batch", "outcome_ids"],
            ));
            values
        }
        ProjectSkillResolutionIdentityKind::Recurrence => {
            provenance_string_values(&skill.provenance_json, &["additional", "recurrence_key"])
        }
        ProjectSkillResolutionIdentityKind::VerificationGap => provenance_string_values(
            &skill.provenance_json,
            &["additional", "verification_gap_fingerprint"],
        ),
        ProjectSkillResolutionIdentityKind::PullRequest => {
            let mut values = provenance_string_values(&skill.provenance_json, &["source_ref_id"]);
            if let Some(number) = skill
                .provenance_json
                .get("pull_request_number")
                .and_then(Value::as_i64)
            {
                values.push(number.to_string());
            }
            values
        }
        ProjectSkillResolutionIdentityKind::ImportExternal
        | ProjectSkillResolutionIdentityKind::SourcePath => {
            provenance_string_values(&skill.provenance_json, &["external_id"])
        }
        ProjectSkillResolutionIdentityKind::ImportTitle => vec![normalized_import_title(
            &skill.title,
            &skill.bucket,
            &skill.stage,
        )],
        ProjectSkillResolutionIdentityKind::Memory => {
            provenance_string_values(&skill.provenance_json, &["memory_id"])
        }
    }
}

fn provenance_string_values(value: &Value, path: &[&str]) -> Vec<String> {
    let mut values = value
        .pointer(&format!("/{}", path.join("/")))
        .and_then(Value::as_str)
        .map(str::to_string)
        .into_iter()
        .collect::<Vec<_>>();
    if let Some(evidence) = value.get("resolution_evidence").and_then(Value::as_array) {
        for item in evidence {
            values.extend(provenance_string_values(item, path));
        }
    }
    values
}

fn provenance_array_string_values(value: &Value, path: &[&str]) -> Vec<String> {
    let mut values = value
        .pointer(&format!("/{}", path.join("/")))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if let Some(evidence) = value.get("resolution_evidence").and_then(Value::as_array) {
        for item in evidence {
            values.extend(provenance_array_string_values(item, path));
        }
    }
    values
}

fn push_provenance_identity(
    identities: &mut Vec<ProjectSkillResolutionIdentity>,
    skill: &ProjectSkill,
    kind: ProjectSkillResolutionIdentityKind,
    path: &[&str],
) {
    for value in provenance_string_values(&skill.provenance_json, path) {
        if !value.trim().is_empty() {
            identities.push(ProjectSkillResolutionIdentity { kind, value });
        }
    }
}

fn project_skill_payload_matches(current: &ProjectSkill, candidate: &ProjectSkill) -> bool {
    current.title == candidate.title
        && current.bucket == candidate.bucket
        && current.stage == candidate.stage
        && current.scope_paths == candidate.scope_paths
        && current.compact_guidance == candidate.compact_guidance
        && current.body_markdown == candidate.body_markdown
        && current.predicted_effect == candidate.predicted_effect
}

fn copy_candidate_content(target: &mut ProjectSkill, candidate: &ProjectSkill) {
    target.title = candidate.title.clone();
    target.bucket = candidate.bucket.clone();
    target.stage = candidate.stage.clone();
    target.scope_paths = candidate.scope_paths.clone();
    target.compact_guidance = candidate.compact_guidance.clone();
    target.body_markdown = candidate.body_markdown.clone();
    target.predicted_effect = candidate.predicted_effect.clone();
}

fn append_resolution_evidence(existing: &mut Value, incoming: &Value) {
    if provenance_contains(existing, incoming) {
        return;
    }
    if !existing.is_object() {
        *existing = Value::Object(Map::new());
    }
    let Some(object) = existing.as_object_mut() else {
        return;
    };
    let evidence = object
        .entry("resolution_evidence")
        .or_insert_with(|| Value::Array(Vec::new()));
    if !evidence.is_array() {
        *evidence = Value::Array(Vec::new());
    }
    if let Some(values) = evidence.as_array_mut() {
        values.push(incoming.clone());
    }
}

fn provenance_contains(existing: &Value, incoming: &Value) -> bool {
    match (existing, incoming) {
        (_, Value::Object(incoming)) if incoming.is_empty() => true,
        (Value::Object(existing), Value::Object(incoming)) => {
            let direct = incoming.iter().all(|(key, value)| {
                existing
                    .get(key)
                    .is_some_and(|existing| provenance_contains(existing, value))
            });
            direct
                || existing
                    .get("resolution_evidence")
                    .and_then(Value::as_array)
                    .is_some_and(|values| {
                        values
                            .iter()
                            .any(|value| value == &Value::Object(incoming.clone()))
                    })
        }
        (Value::Array(existing), Value::Array(incoming)) => incoming
            .iter()
            .all(|value| existing.iter().any(|existing| existing == value)),
        _ => existing == incoming,
    }
}

fn merge_json(target: &mut Value, incoming: &Value) {
    match (target, incoming) {
        (Value::Object(target), Value::Object(incoming)) => {
            for (key, value) in incoming {
                match target.get_mut(key) {
                    Some(target_value) => merge_json(target_value, value),
                    None => {
                        target.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (target, incoming) => *target = incoming.clone(),
    }
}

fn project_skill_source_sync_enabled(skill: &ProjectSkill) -> bool {
    skill
        .provenance_json
        .get("source_sync_enabled")
        .and_then(Value::as_bool)
        .or_else(|| {
            skill
                .provenance_json
                .get("source_snapshot")
                .and_then(|value| value.get("source_sync_enabled"))
                .and_then(Value::as_bool)
        })
        .unwrap_or(false)
}

fn normalized_identity(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn normalized_import_title(title: &str, bucket: &str, stage: &str) -> String {
    format!(
        "{}\n{}\n{}",
        normalized_identity(title),
        normalized_identity(bucket),
        normalized_identity(stage)
    )
}

fn identity_kind_rank(kind: ProjectSkillResolutionIdentityKind) -> u8 {
    match kind {
        ProjectSkillResolutionIdentityKind::Recurrence => 0,
        ProjectSkillResolutionIdentityKind::VerificationGap => 1,
        ProjectSkillResolutionIdentityKind::Outcome => 2,
        ProjectSkillResolutionIdentityKind::PullRequest => 3,
        ProjectSkillResolutionIdentityKind::ImportExternal => 4,
        ProjectSkillResolutionIdentityKind::Memory => 5,
        ProjectSkillResolutionIdentityKind::SourcePath => 6,
        ProjectSkillResolutionIdentityKind::ImportTitle => 7,
        ProjectSkillResolutionIdentityKind::Content => 8,
    }
}
