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
    let baseline = project_skill_content_hash(
        " Review\u{2003}rule ",
        "REVIEW\tAREA",
        "review\nphase",
        "Body\r\n",
    );
    assert_eq!(
        baseline,
        "334e53311ef161fa46b28f9ad9fed3b47fe21e64a4b0610291fe5d57a7b43455"
    );
    assert_eq!(
        baseline,
        project_skill_content_hash("review rule", "review area", "review phase", "Body")
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
fn evidence_hash_canonicalizes_object_keys_and_rejects_malformed_raw_json() {
    let left = json!({"b": [2, {"z": true, "a": null}], "a": 1});
    let right = json!({"a": 1, "b": [2, {"a": null, "z": true}]});
    assert_eq!(
        project_skill_evidence_hash(&left),
        project_skill_evidence_hash(&right)
    );
    assert_eq!(
        project_skill_evidence_hash(&left),
        "7d547b36444e912fc786654bfb5445f24e9743d360da17afc4d89efe36446716"
    );
    assert_eq!(
        project_skill_evidence_hash_from_raw(r#"{"b":2,"a":1}"#).unwrap(),
        project_skill_evidence_hash_from_raw(r#"{"a":1,"b":2}"#).unwrap()
    );
    assert!(project_skill_evidence_hash_from_raw("{malformed").is_err());
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
        project_skill_authorship_from_provenance(&json!({
            "source": "github_pr_history",
            "source_ref_kind": "pull_request"
        })),
        ProjectSkillCreatedBy::User
    );
    assert_eq!(
        project_skill_authorship_from_provenance(&json!({"source": "unknown_legacy"})),
        ProjectSkillCreatedBy::Agent
    );
    assert_eq!(
        project_skill_authorship_from_provenance(
            &json!({"source": "memory_to_project_skill_promotion"})
        ),
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
