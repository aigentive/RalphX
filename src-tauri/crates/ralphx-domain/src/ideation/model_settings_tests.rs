use std::str::FromStr;

use chrono::Utc;

use crate::ideation::model_settings::{
    model_bucket_for_agent, IdeationModelSettings, ModelBucket, ModelLevel,
};

// ── ModelLevel parsing ────────────────────────────────────────────────────

#[test]
fn test_model_level_round_trip() {
    let values = [
        "inherit",
        "sonnet",
        "opus",
        "claude-opus-4-7",
        "claude-opus-4-8",
        "claude-opus-5",
        "haiku",
        "fable",
    ];
    for v in values {
        let parsed = ModelLevel::from_str(v).expect("parse");
        assert_eq!(parsed.to_string(), v, "round-trip for '{}'", v);
    }
}

#[test]
fn test_model_level_invalid() {
    assert!(ModelLevel::from_str("gpt4").is_err());
    assert!(ModelLevel::from_str("").is_err());
    assert!(ModelLevel::from_str("SONNET").is_err());
}

#[test]
fn test_model_level_default_is_inherit() {
    assert_eq!(ModelLevel::default(), ModelLevel::Inherit);
}

#[test]
fn test_model_level_serde() {
    let json = serde_json::to_string(&ModelLevel::Opus).unwrap();
    assert_eq!(json, "\"opus\"");
    let de: ModelLevel = serde_json::from_str("\"haiku\"").unwrap();
    assert_eq!(de, ModelLevel::Haiku);
    let de_fable: ModelLevel = serde_json::from_str("\"fable\"").unwrap();
    assert_eq!(de_fable, ModelLevel::Fable);
    let de_inherit: ModelLevel = serde_json::from_str("\"inherit\"").unwrap();
    assert_eq!(de_inherit, ModelLevel::Inherit);

    for (json, expected) in [
        ("\"claude-opus-4-7\"", ModelLevel::ClaudeOpus47),
        ("\"claude-opus-4-8\"", ModelLevel::ClaudeOpus48),
        ("\"claude-opus-5\"", ModelLevel::ClaudeOpus5),
    ] {
        let parsed: ModelLevel = serde_json::from_str(json).unwrap();
        assert_eq!(parsed, expected);
        assert_eq!(serde_json::to_string(&parsed).unwrap(), json);
    }
}

// ── ModelBucket mapping ───────────────────────────────────────────────────

#[test]
fn test_model_bucket_for_agent_primary() {
    let primary_agents = ["ralphx-ideation", "ralphx-ideation-readonly"];
    for agent in primary_agents {
        assert_eq!(
            model_bucket_for_agent(agent),
            Some(ModelBucket::Primary),
            "agent '{}' should map to Primary",
            agent
        );
    }
}

#[test]
fn test_model_bucket_for_agent_verifier() {
    assert_eq!(
        model_bucket_for_agent("ralphx-plan-verifier"),
        Some(ModelBucket::Verifier)
    );
    assert_eq!(
        model_bucket_for_agent("ralphx:ralphx-plan-verifier"),
        Some(ModelBucket::Verifier)
    );
}

#[test]
fn test_model_bucket_for_agent_primary_fully_qualified() {
    assert_eq!(
        model_bucket_for_agent("ralphx:ralphx-ideation"),
        Some(ModelBucket::Primary)
    );
}

#[test]
fn test_model_bucket_for_agent_legacy_primary_aliases() {
    for agent in ["orchestrator-ideation", "orchestrator-ideation-readonly"] {
        assert_eq!(
            model_bucket_for_agent(agent),
            Some(ModelBucket::Primary),
            "legacy agent '{}' should map to Primary",
            agent
        );
    }
}

#[test]
fn test_model_bucket_for_agent_legacy_verifier_alias() {
    assert_eq!(
        model_bucket_for_agent("plan-verifier"),
        Some(ModelBucket::Verifier)
    );
}

#[test]
fn test_model_bucket_for_agent_none() {
    assert_eq!(model_bucket_for_agent("ralphx-execution-worker"), None);
    assert_eq!(model_bucket_for_agent("ralphx-execution-coder"), None);
    assert_eq!(model_bucket_for_agent(""), None);
    assert_eq!(model_bucket_for_agent("unknown-agent"), None);
}

// ── model_for_bucket() method ─────────────────────────────────────────────

#[test]
fn test_model_for_bucket_primary() {
    let settings = IdeationModelSettings {
        id: 1,
        project_id: None,
        primary_model: ModelLevel::Opus,
        verifier_model: ModelLevel::Sonnet,
        verifier_subagent_model: ModelLevel::Inherit,
        ideation_subagent_model: ModelLevel::Inherit,
        updated_at: Utc::now(),
    };
    assert_eq!(
        settings.model_for_bucket(&ModelBucket::Primary),
        &ModelLevel::Opus
    );
}

#[test]
fn test_model_for_bucket_verifier() {
    let settings = IdeationModelSettings {
        id: 1,
        project_id: None,
        primary_model: ModelLevel::Opus,
        verifier_model: ModelLevel::Haiku,
        verifier_subagent_model: ModelLevel::Inherit,
        ideation_subagent_model: ModelLevel::Inherit,
        updated_at: Utc::now(),
    };
    assert_eq!(
        settings.model_for_bucket(&ModelBucket::Verifier),
        &ModelLevel::Haiku
    );
}

#[test]
fn test_model_for_bucket_inherit() {
    let settings = IdeationModelSettings {
        id: 1,
        project_id: None,
        primary_model: ModelLevel::Inherit,
        verifier_model: ModelLevel::Inherit,
        verifier_subagent_model: ModelLevel::Inherit,
        ideation_subagent_model: ModelLevel::Inherit,
        updated_at: Utc::now(),
    };
    assert_eq!(
        settings.model_for_bucket(&ModelBucket::Primary),
        &ModelLevel::Inherit
    );
    assert_eq!(
        settings.model_for_bucket(&ModelBucket::Verifier),
        &ModelLevel::Inherit
    );
}

#[test]
fn test_model_for_bucket_verifier_subagent() {
    let settings = IdeationModelSettings {
        id: 1,
        project_id: None,
        primary_model: ModelLevel::Opus,
        verifier_model: ModelLevel::Sonnet,
        verifier_subagent_model: ModelLevel::Haiku,
        ideation_subagent_model: ModelLevel::Inherit,
        updated_at: Utc::now(),
    };
    assert_eq!(
        settings.model_for_bucket(&ModelBucket::VerifierSubagent),
        &ModelLevel::Haiku
    );
}

#[test]
fn test_verifier_subagent_bucket_not_in_agent_map() {
    // VerifierSubagent is a cap-resolution bucket, not an agent bucket.
    // No agent name should map to it via model_bucket_for_agent().
    let agents = [
        "ralphx-ideation",
        "ralphx-ideation-readonly",
        "ralphx-plan-verifier",
        "ralphx:ralphx-plan-verifier",
        "ralphx-execution-worker",
        "ralphx-execution-coder",
        "",
    ];
    for agent in agents {
        assert_ne!(
            model_bucket_for_agent(agent),
            Some(ModelBucket::VerifierSubagent),
            "agent '{}' should NOT map to VerifierSubagent",
            agent
        );
        assert_ne!(
            model_bucket_for_agent(agent),
            Some(ModelBucket::IdeationSubagent),
            "agent '{}' should NOT map to IdeationSubagent",
            agent
        );
    }
}
