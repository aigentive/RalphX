use std::str::FromStr;

use serde_json::json;

use super::project_skill_versioning::{
    project_skill_authorship_from_provenance, project_skill_content_hash,
    project_skill_evidence_hash, project_skill_evidence_hash_from_raw,
    project_skill_pipeline_role_from_provenance, ProjectSkillCreatedBy,
};

#[test]
fn project_skill_authorship_accepts_only_the_persisted_vocabulary() {
    for (raw, expected) in [
        ("user", ProjectSkillCreatedBy::User),
        ("agent", ProjectSkillCreatedBy::Agent),
        ("imported", ProjectSkillCreatedBy::Imported),
    ] {
        assert_eq!(ProjectSkillCreatedBy::from_str(raw).unwrap(), expected);
        assert_eq!(expected.to_string(), raw);
    }
    assert!(ProjectSkillCreatedBy::from_str("system").is_err());
}

#[test]
fn content_hash_is_stable_and_sensitive_to_each_identity_input() {
    let baseline = project_skill_content_hash(" Review rule ", "REVIEW", "review", "Body\r\n");
    assert_eq!(
        baseline,
        project_skill_content_hash("review rule", "review", "review", "Body")
    );
    assert_ne!(
        baseline,
        project_skill_content_hash("different", "review", "review", "Body")
    );
    assert_ne!(
        baseline,
        project_skill_content_hash("review rule", "merge", "review", "Body")
    );
    assert_ne!(
        baseline,
        project_skill_content_hash("review rule", "review", "merge", "Body")
    );
    assert_ne!(
        baseline,
        project_skill_content_hash("review rule", "review", "review", "Different")
    );
}

#[test]
fn evidence_hash_canonicalizes_object_keys_and_preserves_raw_fallback_identity() {
    let left = json!({"b": [2, {"z": true, "a": null}], "a": 1});
    let right = json!({"a": 1, "b": [2, {"a": null, "z": true}]});
    assert_eq!(
        project_skill_evidence_hash(&left),
        project_skill_evidence_hash(&right)
    );
    assert_eq!(
        project_skill_evidence_hash_from_raw(r#"{"b":2,"a":1}"#),
        project_skill_evidence_hash_from_raw(r#"{"a":1,"b":2}"#)
    );
    assert_eq!(
        project_skill_evidence_hash_from_raw("{malformed"),
        project_skill_evidence_hash_from_raw("{malformed")
    );
    assert_ne!(
        project_skill_evidence_hash_from_raw("{malformed"),
        project_skill_evidence_hash_from_raw("{different")
    );
}

#[test]
fn provenance_backfill_is_conservative_and_pipeline_role_is_trimmed() {
    assert_eq!(
        project_skill_authorship_from_provenance(&json!({"source": "project_skill_import"})),
        ProjectSkillCreatedBy::Imported
    );
    assert_eq!(
        project_skill_authorship_from_provenance(&json!({"source": "task_outcome"})),
        ProjectSkillCreatedBy::Agent
    );
    assert_eq!(
        project_skill_authorship_from_provenance(&json!({"source": "github_pr_history"})),
        ProjectSkillCreatedBy::Agent
    );
    assert_eq!(
        project_skill_authorship_from_provenance(&json!({"source": "unknown_legacy"})),
        ProjectSkillCreatedBy::User
    );
    assert_eq!(
        project_skill_pipeline_role_from_provenance(
            &json!({"additional": {"pipeline_role": " verifier "}})
        ),
        Some("verifier".to_string())
    );
    assert_eq!(
        project_skill_pipeline_role_from_provenance(
            &json!({"additional": {"pipeline_role": "   "}})
        ),
        None
    );
}
