use super::*;
use crate::infrastructure::agents::claude::agent_config::{
    parse_config_no_env_overrides, EMBEDDED_CONFIG,
};

// ── Model tier tests ────────────────────────────────────────────

#[test]
fn test_model_within_cap_haiku_under_sonnet() {
    assert!(model_within_cap("haiku", "sonnet"));
}

#[test]
fn test_model_within_cap_sonnet_equals_sonnet() {
    assert!(model_within_cap("sonnet", "sonnet"));
}

#[test]
fn test_model_within_cap_opus_exceeds_sonnet() {
    assert!(!model_within_cap("opus", "sonnet"));
}

#[test]
fn test_model_within_cap_opus_under_opus() {
    assert!(model_within_cap("opus", "opus"));
}

#[test]
fn test_model_within_cap_fable_above_opus() {
    assert!(!model_within_cap("fable", "opus"));
    assert!(model_within_cap("opus", "fable"));
    assert!(model_within_cap("claude-fable-5", "fable"));
}

#[test]
fn test_model_within_cap_unknown_model_always_within() {
    assert!(model_within_cap("unknown", "haiku"));
}

#[test]
fn test_model_within_cap_case_insensitive() {
    assert!(model_within_cap("Haiku", "SONNET"));
}

// ── ProcessSlot deserialization tests ────────────────────────────

#[test]
fn test_process_slot_deserialize_with_variants() {
    let yaml = r#"
default: ralphx-execution-worker
team: ralphx-execution-team-lead
"#;
    let slot: ProcessSlot = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(slot.default, "ralphx-execution-worker");
    assert_eq!(slot.variants.get("team").unwrap(), "ralphx-execution-team-lead");
}

#[test]
fn test_process_slot_deserialize_default_only() {
    let yaml = "default: ralphx-execution-merger\n";
    let slot: ProcessSlot = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(slot.default, "ralphx-execution-merger");
    assert!(slot.variants.is_empty());
}

// ── ProcessMapping deserialization tests ─────────────────────────

#[test]
fn test_process_mapping_deserialize_full() {
    let yaml = r#"
ideation:
  default: ralphx-ideation
  readonly: ralphx-ideation-readonly
  team: ralphx-ideation-team-lead
execution:
  default: ralphx-execution-worker
  team: ralphx-execution-team-lead
merge:
  default: ralphx-execution-merger
"#;
    let mapping: ProcessMapping = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(mapping.slots.len(), 3);
    assert_eq!(mapping.slots["ideation"].default, "ralphx-ideation");
    assert_eq!(
        mapping.slots["ideation"].variants.get("team").unwrap(),
        "ralphx-ideation-team-lead"
    );
    assert_eq!(mapping.slots["execution"].default, "ralphx-execution-worker");
    assert_eq!(mapping.slots["merge"].default, "ralphx-execution-merger");
}

#[test]
fn test_process_mapping_empty_is_default() {
    let mapping = ProcessMapping::default();
    assert!(mapping.slots.is_empty());
}

#[test]
fn test_canonical_process_mapping_contains_live_process_slots() {
    let mapping = canonical_process_mapping();
    assert_eq!(mapping.slots["ideation"].default, "ralphx-ideation");
    assert_eq!(
        mapping.slots["ideation"].variants.get("team").map(String::as_str),
        Some("ralphx-ideation-team-lead")
    );
    assert_eq!(
        mapping.slots["review"].variants.get("history").map(String::as_str),
        Some("ralphx-review-history")
    );
    assert_eq!(mapping.slots["chat_project"].default, "ralphx-chat-project");
}

#[test]
fn test_resolve_canonical_process_mapping_preserves_unknown_yaml_slots() {
    let mut raw = ProcessMapping::default();
    raw.slots.insert(
        "custom_process".to_string(),
        ProcessSlot {
            default: "custom-agent".to_string(),
            variants: HashMap::new(),
        },
    );

    let resolved = resolve_canonical_process_mapping(&raw);
    assert_eq!(
        resolved.slots["custom_process"].default,
        "custom-agent",
        "unknown YAML-only process slots should be preserved"
    );
    assert_eq!(resolved.slots["execution"].default, "ralphx-execution-worker");
}

#[test]
fn test_embedded_config_omits_process_mapping_and_still_resolves_canonical_registry() {
    let parsed = parse_config_no_env_overrides(EMBEDDED_CONFIG).expect("embedded config should parse");

    assert_eq!(
        parsed.process_mapping,
        canonical_process_mapping(),
        "embedded config/ralphx.yaml should be able to omit process_mapping while the loader still resolves the canonical registry"
    );
}

#[test]
fn test_canonical_team_constraints_contains_live_process_defaults() {
    let config = canonical_team_constraints_config();
    let defaults = config.defaults.as_ref().expect("defaults should exist");
    assert_eq!(defaults.max_teammates, 5);
    assert_eq!(defaults.model_cap, "sonnet");
    assert_eq!(defaults.auto_approve, Some(true));
    assert_eq!(config.processes["execution"].timeout_minutes, 30);
    assert_eq!(config.processes["review"].mode, TeamMode::Constrained);
}

#[test]
fn test_resolve_canonical_team_constraints_preserves_unknown_yaml_processes() {
    let mut raw = TeamConstraintsConfig::default();
    raw.processes.insert(
        "custom_process".to_string(),
        TeamConstraints {
            max_teammates: 2,
            allowed_tools: vec!["Read".to_string()],
            allowed_mcp_tools: vec!["get_task_context".to_string()],
            model_cap: "haiku".to_string(),
            mode: TeamMode::Constrained,
            presets: vec!["custom-agent".to_string()],
            timeout_minutes: 15,
            budget_limit: Some(1.0),
            auto_approve: Some(false),
        },
    );

    let resolved = resolve_canonical_team_constraints_config(&raw);
    assert_eq!(resolved.processes["custom_process"].model_cap, "haiku");
    assert_eq!(resolved.processes["execution"].model_cap, "sonnet");
}

#[test]
fn test_embedded_config_omits_team_constraints_and_still_resolves_canonical_registry() {
    let parsed = parse_config_no_env_overrides(EMBEDDED_CONFIG).expect("embedded config should parse");

    assert_eq!(
        parsed.team_constraints,
        canonical_team_constraints_config(),
        "embedded config/ralphx.yaml should be able to omit team_constraints while the loader still resolves the canonical team constraint registry"
    );
}

#[test]
fn test_processes_config_file_process_mapping_matches_canonical_registry() {
    #[derive(serde::Deserialize)]
    struct ProcessesConfigMirror {
        #[serde(default)]
        process_mapping: ProcessMapping,
    }

    let yaml_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../config/processes.yaml");
    let contents = std::fs::read_to_string(&yaml_path).expect("should read config/processes.yaml");
    let parsed: ProcessesConfigMirror =
        serde_yaml::from_str(&contents).expect("should parse config/processes.yaml");

    assert_eq!(
        parsed.process_mapping,
        canonical_process_mapping(),
        "config/processes.yaml process_mapping should stay aligned with the canonical process registry"
    );
}

#[test]
fn test_processes_config_file_team_constraints_match_canonical_registry() {
    #[derive(serde::Deserialize)]
    struct ProcessesConfigMirror {
        #[serde(default)]
        team_constraints: TeamConstraintsConfig,
    }

    let yaml_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../config/processes.yaml");
    let contents = std::fs::read_to_string(&yaml_path).expect("should read config/processes.yaml");
    let parsed: ProcessesConfigMirror =
        serde_yaml::from_str(&contents).expect("should parse config/processes.yaml");

    assert_eq!(
        parsed.team_constraints,
        canonical_team_constraints_config(),
        "config/processes.yaml team_constraints should stay aligned with the canonical team constraint registry"
    );
}

// ── TeamConstraints deserialization tests ────────────────────────

#[test]
fn test_team_constraints_defaults() {
    let tc = TeamConstraints::default();
    assert_eq!(tc.max_teammates, 5);
    assert_eq!(tc.model_cap, "sonnet");
    assert_eq!(tc.mode, TeamMode::Dynamic);
    assert_eq!(tc.timeout_minutes, 20);
    assert!(tc.budget_limit.is_none());
    assert!(tc.allowed_tools.is_empty());
    assert!(tc.presets.is_empty());
    assert!(tc.auto_approve.is_none());
}

#[test]
fn test_team_constraints_deserialize_full() {
    let yaml = r#"
max_teammates: 3
allowed_tools: [Read, Write, Edit]
allowed_mcp_tools: [get_task_context]
model_cap: opus
mode: constrained
presets: [ralphx-execution-coder, ralphx-execution-reviewer]
timeout_minutes: 45
budget_limit: 10.50
"#;
    let tc: TeamConstraints = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tc.max_teammates, 3);
    assert_eq!(tc.allowed_tools, vec!["Read", "Write", "Edit"]);
    assert_eq!(tc.allowed_mcp_tools, vec!["get_task_context"]);
    assert_eq!(tc.model_cap, "opus");
    assert_eq!(tc.mode, TeamMode::Constrained);
    assert_eq!(tc.presets, vec!["ralphx-execution-coder", "ralphx-execution-reviewer"]);
    assert_eq!(tc.timeout_minutes, 45);
    assert_eq!(tc.budget_limit, Some(10.50));
}

#[test]
fn test_team_constraints_deserialize_partial_uses_defaults() {
    let yaml = "max_teammates: 2\n";
    let tc: TeamConstraints = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tc.max_teammates, 2);
    assert_eq!(tc.model_cap, "sonnet"); // default
    assert_eq!(tc.mode, TeamMode::Dynamic); // default
    assert_eq!(tc.timeout_minutes, 20); // default
}

// ── TeamConstraintsConfig deserialization tests ──────────────────

#[test]
fn test_team_constraints_config_with_defaults_and_processes() {
    let yaml = r#"
_defaults:
  max_teammates: 5
  model_cap: sonnet
  mode: dynamic
  timeout_minutes: 20
execution:
  max_teammates: 5
  allowed_tools: [Read, Write, Edit, Bash]
  model_cap: sonnet
  mode: dynamic
  timeout_minutes: 30
review:
  max_teammates: 2
  mode: constrained
  presets: [ralphx-execution-reviewer]
"#;
    let config: TeamConstraintsConfig = serde_yaml::from_str(yaml).unwrap();
    assert!(config.defaults.is_some());
    assert_eq!(config.processes.len(), 2);
    assert_eq!(config.processes["execution"].max_teammates, 5);
    assert_eq!(config.processes["review"].mode, TeamMode::Constrained);
}

#[test]
fn test_team_constraints_config_empty_is_default() {
    let config = TeamConstraintsConfig::default();
    assert!(config.defaults.is_none());
    assert!(config.processes.is_empty());
}

// ── get_team_constraints tests ──────────────────────────────────

#[test]
fn test_get_team_constraints_process_specific_found() {
    let config = TeamConstraintsConfig {
        defaults: Some(TeamConstraints {
            max_teammates: 10,
            ..TeamConstraints::default()
        }),
        processes: {
            let mut m = HashMap::new();
            m.insert(
                "execution".to_string(),
                TeamConstraints {
                    max_teammates: 3,
                    allowed_tools: vec!["Read".to_string()],
                    ..TeamConstraints::default()
                },
            );
            m
        },
    };
    let tc = get_team_constraints(&config, "execution");
    assert_eq!(tc.max_teammates, 3);
    assert_eq!(tc.allowed_tools, vec!["Read"]);
}

#[test]
fn test_get_team_constraints_falls_back_to_defaults() {
    let config = TeamConstraintsConfig {
        defaults: Some(TeamConstraints {
            max_teammates: 7,
            model_cap: "opus".to_string(),
            ..TeamConstraints::default()
        }),
        processes: HashMap::new(),
    };
    let tc = get_team_constraints(&config, "ideation");
    assert_eq!(tc.max_teammates, 7);
    assert_eq!(tc.model_cap, "opus");
}

#[test]
fn test_get_team_constraints_no_config_uses_hardcoded_defaults() {
    let config = TeamConstraintsConfig::default();
    let tc = get_team_constraints(&config, "execution");
    assert_eq!(tc.max_teammates, 5);
    assert_eq!(tc.model_cap, "sonnet");
    assert_eq!(tc.mode, TeamMode::Dynamic);
}

#[test]
fn test_merge_constraints_specific_tools_override_defaults() {
    let defaults = TeamConstraints {
        allowed_tools: vec!["Read".to_string(), "Grep".to_string()],
        ..TeamConstraints::default()
    };
    let specific = TeamConstraints {
        allowed_tools: vec!["Write".to_string()],
        ..TeamConstraints::default()
    };
    let merged = merge_constraints(&defaults, &specific);
    assert_eq!(merged.allowed_tools, vec!["Write"]);
}

#[test]
fn test_merge_constraints_empty_specific_tools_inherits_defaults() {
    let defaults = TeamConstraints {
        allowed_tools: vec!["Read".to_string(), "Grep".to_string()],
        ..TeamConstraints::default()
    };
    let specific = TeamConstraints {
        max_teammates: 2,
        allowed_tools: Vec::new(),
        ..TeamConstraints::default()
    };
    let merged = merge_constraints(&defaults, &specific);
    assert_eq!(merged.allowed_tools, vec!["Read", "Grep"]);
    assert_eq!(merged.max_teammates, 2);
}

// ── resolve_process_agent tests ─────────────────────────────────

#[test]
fn test_resolve_process_agent_default_variant() {
    let mapping = build_test_mapping();
    let agent = resolve_process_agent(&mapping, "execution", "default");
    assert_eq!(agent, Some("ralphx-execution-worker".to_string()));
}

#[test]
fn test_resolve_process_agent_team_variant() {
    let mapping = build_test_mapping();
    let agent = resolve_process_agent(&mapping, "execution", "team");
    assert_eq!(agent, Some("ralphx-execution-team-lead".to_string()));
}

#[test]
fn test_resolve_process_agent_unknown_variant_falls_back_to_default() {
    let mapping = build_test_mapping();
    let agent = resolve_process_agent(&mapping, "execution", "nonexistent");
    assert_eq!(agent, Some("ralphx-execution-worker".to_string()));
}

#[test]
fn test_resolve_process_agent_unknown_process_returns_none() {
    let mapping = build_test_mapping();
    let agent = resolve_process_agent(&mapping, "unknown_process", "default");
    assert_eq!(agent, None);
}

#[test]
fn test_resolve_process_agent_empty_mapping_returns_none() {
    let mapping = ProcessMapping::default();
    assert_eq!(
        resolve_process_agent(&mapping, "execution", "default"),
        None
    );
}

// ── validate_team_plan tests ────────────────────────────────────

#[test]
fn test_validate_team_plan_dynamic_valid() {
    let constraints = TeamConstraints {
        max_teammates: 3,
        allowed_tools: vec!["Read".to_string(), "Write".to_string(), "Grep".to_string()],
        allowed_mcp_tools: vec!["get_task_context".to_string()],
        model_cap: "sonnet".to_string(),
        mode: TeamMode::Dynamic,
        ..TeamConstraints::default()
    };
    let teammates = vec![
        TeammateSpawnRequest {
            role: "coder".to_string(),
            tools: vec!["Read".to_string(), "Write".to_string()],
            mcp_tools: vec!["get_task_context".to_string()],
            model: "sonnet".to_string(),
            ..default_spawn_request()
        },
        TeammateSpawnRequest {
            role: "tester".to_string(),
            tools: vec!["Read".to_string(), "Grep".to_string()],
            mcp_tools: vec![],
            model: "haiku".to_string(),
            ..default_spawn_request()
        },
    ];

    let plan = validate_team_plan(&constraints, "execution", &teammates).unwrap();
    assert_eq!(plan.process, "execution");
    assert_eq!(plan.teammates.len(), 2);
    assert!(!plan.teammates[0].from_preset);
    assert_eq!(plan.teammates[0].approved_model, "sonnet");
    assert_eq!(plan.teammates[1].approved_model, "haiku");
}

#[test]
fn test_validate_team_plan_exceeds_max_teammates() {
    let constraints = TeamConstraints {
        max_teammates: 1,
        mode: TeamMode::Dynamic,
        ..TeamConstraints::default()
    };
    let teammates = vec![
        TeammateSpawnRequest {
            role: "a".to_string(),
            ..default_spawn_request()
        },
        TeammateSpawnRequest {
            role: "b".to_string(),
            ..default_spawn_request()
        },
    ];
    let err = validate_team_plan(&constraints, "execution", &teammates).unwrap_err();
    assert_eq!(
        err,
        TeamConstraintError::MaxTeammatesExceeded {
            max: 1,
            requested: 2
        }
    );
}

#[test]
fn test_validate_team_plan_tool_not_allowed() {
    let constraints = TeamConstraints {
        max_teammates: 5,
        allowed_tools: vec!["Read".to_string()],
        mode: TeamMode::Dynamic,
        ..TeamConstraints::default()
    };
    let teammates = vec![TeammateSpawnRequest {
        role: "coder".to_string(),
        tools: vec!["Read".to_string(), "Write".to_string()],
        ..default_spawn_request()
    }];
    let err = validate_team_plan(&constraints, "execution", &teammates).unwrap_err();
    assert_eq!(
        err,
        TeamConstraintError::ToolNotAllowed {
            tool: "Write".to_string(),
            role: "coder".to_string(),
        }
    );
}

#[test]
fn test_validate_team_plan_mcp_tool_not_allowed() {
    let constraints = TeamConstraints {
        max_teammates: 5,
        allowed_mcp_tools: vec!["get_task_context".to_string()],
        mode: TeamMode::Dynamic,
        ..TeamConstraints::default()
    };
    let teammates = vec![TeammateSpawnRequest {
        role: "coder".to_string(),
        mcp_tools: vec!["start_step".to_string()],
        ..default_spawn_request()
    }];
    let err = validate_team_plan(&constraints, "execution", &teammates).unwrap_err();
    assert_eq!(
        err,
        TeamConstraintError::McpToolNotAllowed {
            tool: "start_step".to_string(),
            role: "coder".to_string(),
        }
    );
}

#[test]
fn test_validate_team_plan_model_exceeds_cap() {
    let constraints = TeamConstraints {
        max_teammates: 5,
        model_cap: "sonnet".to_string(),
        mode: TeamMode::Dynamic,
        ..TeamConstraints::default()
    };
    let teammates = vec![TeammateSpawnRequest {
        role: "researcher".to_string(),
        model: "opus".to_string(),
        ..default_spawn_request()
    }];
    let err = validate_team_plan(&constraints, "execution", &teammates).unwrap_err();
    assert_eq!(
        err,
        TeamConstraintError::ModelExceedsCap {
            requested: "opus".to_string(),
            cap: "sonnet".to_string(),
        }
    );
}

#[test]
fn test_validate_team_plan_dynamic_empty_allowed_tools_permits_all() {
    // When allowed_tools is empty, no tool restriction is enforced
    let constraints = TeamConstraints {
        max_teammates: 5,
        allowed_tools: vec![],
        mode: TeamMode::Dynamic,
        ..TeamConstraints::default()
    };
    let teammates = vec![TeammateSpawnRequest {
        role: "coder".to_string(),
        tools: vec!["Read".to_string(), "Write".to_string(), "Edit".to_string()],
        ..default_spawn_request()
    }];
    assert!(validate_team_plan(&constraints, "execution", &teammates).is_ok());
}

#[test]
fn test_validate_team_plan_constrained_valid_preset() {
    let constraints = TeamConstraints {
        max_teammates: 3,
        mode: TeamMode::Constrained,
        presets: vec!["ralphx-execution-coder".to_string(), "ralphx-execution-reviewer".to_string()],
        ..TeamConstraints::default()
    };
    let teammates = vec![TeammateSpawnRequest {
        role: "coder".to_string(),
        preset: Some("ralphx-execution-coder".to_string()),
        ..default_spawn_request()
    }];
    let plan = validate_team_plan(&constraints, "execution", &teammates).unwrap();
    assert!(plan.teammates[0].from_preset);
}

#[test]
fn test_validate_team_plan_constrained_no_preset_fails() {
    let constraints = TeamConstraints {
        max_teammates: 3,
        mode: TeamMode::Constrained,
        presets: vec!["ralphx-execution-coder".to_string()],
        ..TeamConstraints::default()
    };
    let teammates = vec![TeammateSpawnRequest {
        role: "coder".to_string(),
        preset: None,
        ..default_spawn_request()
    }];
    let err = validate_team_plan(&constraints, "execution", &teammates).unwrap_err();
    assert_eq!(
        err,
        TeamConstraintError::PresetRequired {
            role: "coder".to_string()
        }
    );
}

#[test]
fn test_validate_team_plan_constrained_unknown_preset_fails() {
    let constraints = TeamConstraints {
        max_teammates: 3,
        mode: TeamMode::Constrained,
        presets: vec!["ralphx-execution-coder".to_string()],
        ..TeamConstraints::default()
    };
    let teammates = vec![TeammateSpawnRequest {
        role: "hacker".to_string(),
        preset: Some("unknown-agent".to_string()),
        ..default_spawn_request()
    }];
    let err = validate_team_plan(&constraints, "execution", &teammates).unwrap_err();
    assert_eq!(
        err,
        TeamConstraintError::AgentNotInPresets {
            agent: "unknown-agent".to_string(),
            allowed: vec!["ralphx-execution-coder".to_string()],
        }
    );
}

#[test]
fn test_validate_team_plan_empty_teammates_ok() {
    let constraints = TeamConstraints::default();
    let plan = validate_team_plan(&constraints, "execution", &[]).unwrap();
    assert!(plan.teammates.is_empty());
}

// ── Environment variable override tests ─────────────────────────

#[test]
fn test_env_override_team_mode() {
    let mut tc = TeamConstraints::default();
    apply_env_overrides_with(&mut tc, "execution", &|name| match name {
        "RALPHX_TEAM_MODE_EXECUTION" => Some("constrained".to_string()),
        _ => None,
    });
    assert_eq!(tc.mode, TeamMode::Constrained);
}

#[test]
fn test_env_override_max_teammates() {
    let mut tc = TeamConstraints::default();
    apply_env_overrides_with(&mut tc, "execution", &|name| match name {
        "RALPHX_TEAM_MAX_EXECUTION" => Some("8".to_string()),
        _ => None,
    });
    assert_eq!(tc.max_teammates, 8);
}

#[test]
fn test_env_override_model_cap() {
    let mut tc = TeamConstraints::default();
    apply_env_overrides_with(&mut tc, "execution", &|name| match name {
        "RALPHX_TEAM_MODEL_CAP_EXECUTION" => Some("opus".to_string()),
        _ => None,
    });
    assert_eq!(tc.model_cap, "opus");
}

#[test]
fn test_env_override_invalid_model_cap_ignored() {
    let mut tc = TeamConstraints::default();
    apply_env_overrides_with(&mut tc, "execution", &|name| match name {
        "RALPHX_TEAM_MODEL_CAP_EXECUTION" => Some("invalid".to_string()),
        _ => None,
    });
    assert_eq!(tc.model_cap, "sonnet"); // unchanged
}

#[test]
fn test_env_variant_override() {
    let variant = env_variant_override_with("execution", &|name| match name {
        "RALPHX_PROCESS_VARIANT_EXECUTION" => Some("team".to_string()),
        _ => None,
    });
    assert_eq!(variant.as_deref(), Some("team"));
}

#[test]
fn test_env_variant_override_blank_is_none() {
    let variant = env_variant_override_with("execution", &|name| match name {
        "RALPHX_PROCESS_VARIANT_EXECUTION" => Some("  ".to_string()),
        _ => None,
    });
    assert_eq!(variant, None);
}

#[test]
fn test_env_variant_override_missing_is_none() {
    let variant = env_variant_override_with("execution", &|_| None);
    assert_eq!(variant, None);
}

// ── TeamMode serde tests ────────────────────────────────────────

#[test]
fn test_team_mode_deserialize_dynamic() {
    let mode: TeamMode = serde_yaml::from_str("dynamic").unwrap();
    assert_eq!(mode, TeamMode::Dynamic);
}

#[test]
fn test_team_mode_deserialize_constrained() {
    let mode: TeamMode = serde_yaml::from_str("constrained").unwrap();
    assert_eq!(mode, TeamMode::Constrained);
}

#[test]
fn test_team_mode_serialize_roundtrip() {
    let json = serde_json::to_string(&TeamMode::Dynamic).unwrap();
    assert_eq!(json, "\"dynamic\"");
    let json = serde_json::to_string(&TeamMode::Constrained).unwrap();
    assert_eq!(json, "\"constrained\"");
}

// ── TeamConstraintError display tests ───────────────────────────

#[test]
fn test_constraint_error_display() {
    let err = TeamConstraintError::MaxTeammatesExceeded {
        max: 3,
        requested: 5,
    };
    assert_eq!(err.to_string(), "Max teammates exceeded: 5 > 3");
}

// ── min_model_cap tests ──────────────────────────────────────────

#[test]
fn test_min_model_cap_haiku_vs_sonnet() {
    assert_eq!(min_model_cap("haiku", "sonnet"), "haiku");
    assert_eq!(min_model_cap("sonnet", "haiku"), "haiku");
}

#[test]
fn test_min_model_cap_sonnet_vs_opus() {
    assert_eq!(min_model_cap("sonnet", "opus"), "sonnet");
    assert_eq!(min_model_cap("opus", "sonnet"), "sonnet");
}

#[test]
fn test_min_model_cap_opus_vs_fable() {
    assert_eq!(min_model_cap("opus", "fable"), "opus");
    assert_eq!(min_model_cap("fable", "opus"), "opus");
}

#[test]
fn test_min_model_cap_haiku_vs_opus() {
    assert_eq!(min_model_cap("haiku", "opus"), "haiku");
    assert_eq!(min_model_cap("opus", "haiku"), "haiku");
}

#[test]
fn test_min_model_cap_equal_models() {
    assert_eq!(min_model_cap("sonnet", "sonnet"), "sonnet");
    assert_eq!(min_model_cap("opus", "opus"), "opus");
    assert_eq!(min_model_cap("haiku", "haiku"), "haiku");
    assert_eq!(min_model_cap("fable", "fable"), "fable");
}

#[test]
fn test_min_model_cap_case_insensitive() {
    assert_eq!(min_model_cap("HAIKU", "Sonnet"), "haiku");
    assert_eq!(min_model_cap("OPUS", "sonnet"), "sonnet");
}

#[test]
fn test_min_model_cap_unknown_treated_as_lowest() {
    // Unknown models have tier 0 (lowest), so the known model wins
    assert_eq!(min_model_cap("unknown", "haiku"), "unknown");
    assert_eq!(min_model_cap("haiku", "unknown"), "unknown");
}

// ── validate_child_team_config tests ─────────────────────────────

#[test]
fn test_validate_child_team_config_caps_max_teammates() {
    let resolved = TeamConstraints {
        max_teammates: 5,
        ..TeamConstraints::default()
    };
    let ceiling = TeamConstraints {
        max_teammates: 3,
        ..TeamConstraints::default()
    };
    let capped = validate_child_team_config(&resolved, &ceiling);
    assert_eq!(capped.max_teammates, 3);
}

#[test]
fn test_validate_child_team_config_caps_model_cap() {
    let resolved = TeamConstraints {
        model_cap: "opus".to_string(),
        ..TeamConstraints::default()
    };
    let ceiling = TeamConstraints {
        model_cap: "sonnet".to_string(),
        ..TeamConstraints::default()
    };
    let capped = validate_child_team_config(&resolved, &ceiling);
    assert_eq!(capped.model_cap, "sonnet");
}

#[test]
fn test_validate_child_team_config_caps_model_cap_parent_higher() {
    let resolved = TeamConstraints {
        model_cap: "haiku".to_string(),
        ..TeamConstraints::default()
    };
    let ceiling = TeamConstraints {
        model_cap: "opus".to_string(),
        ..TeamConstraints::default()
    };
    let capped = validate_child_team_config(&resolved, &ceiling);
    assert_eq!(capped.model_cap, "haiku");
}

#[test]
fn test_validate_child_team_config_intersects_allowed_tools() {
    let resolved = TeamConstraints {
        allowed_tools: vec!["Read".to_string(), "Write".to_string(), "Edit".to_string()],
        ..TeamConstraints::default()
    };
    let ceiling = TeamConstraints {
        allowed_tools: vec!["Read".to_string(), "Grep".to_string()],
        ..TeamConstraints::default()
    };
    let capped = validate_child_team_config(&resolved, &ceiling);
    assert_eq!(capped.allowed_tools, vec!["Read"]);
}

#[test]
fn test_validate_child_team_config_intersects_allowed_mcp_tools() {
    let resolved = TeamConstraints {
        allowed_mcp_tools: vec!["get_task_context".to_string(), "start_step".to_string()],
        ..TeamConstraints::default()
    };
    let ceiling = TeamConstraints {
        allowed_mcp_tools: vec!["get_task_context".to_string(), "complete_step".to_string()],
        ..TeamConstraints::default()
    };
    let capped = validate_child_team_config(&resolved, &ceiling);
    assert_eq!(capped.allowed_mcp_tools, vec!["get_task_context"]);
}

#[test]
fn test_validate_child_team_config_intersects_presets() {
    let resolved = TeamConstraints {
        presets: vec!["ralphx-execution-coder".to_string(), "ralphx-execution-reviewer".to_string()],
        ..TeamConstraints::default()
    };
    let ceiling = TeamConstraints {
        presets: vec!["ralphx-execution-coder".to_string()],
        ..TeamConstraints::default()
    };
    let capped = validate_child_team_config(&resolved, &ceiling);
    assert_eq!(capped.presets, vec!["ralphx-execution-coder"]);
}

#[test]
fn test_validate_child_team_config_presets_no_intersection() {
    let resolved = TeamConstraints {
        presets: vec!["ralphx-execution-reviewer".to_string()],
        ..TeamConstraints::default()
    };
    let ceiling = TeamConstraints {
        presets: vec!["ralphx-execution-coder".to_string()],
        ..TeamConstraints::default()
    };
    let capped = validate_child_team_config(&resolved, &ceiling);
    assert!(capped.presets.is_empty());
}

#[test]
fn test_validate_child_team_config_caps_timeout() {
    let resolved = TeamConstraints {
        timeout_minutes: 60,
        ..TeamConstraints::default()
    };
    let ceiling = TeamConstraints {
        timeout_minutes: 30,
        ..TeamConstraints::default()
    };
    let capped = validate_child_team_config(&resolved, &ceiling);
    assert_eq!(capped.timeout_minutes, 30);
}

#[test]
fn test_validate_child_team_config_caps_budget_some_vs_some() {
    let resolved = TeamConstraints {
        budget_limit: Some(50.0),
        ..TeamConstraints::default()
    };
    let ceiling = TeamConstraints {
        budget_limit: Some(30.0),
        ..TeamConstraints::default()
    };
    let capped = validate_child_team_config(&resolved, &ceiling);
    assert_eq!(capped.budget_limit, Some(30.0));
}

#[test]
fn test_validate_child_team_config_caps_budget_some_vs_none() {
    // If ceiling has no budget limit, pass through resolved
    let resolved = TeamConstraints {
        budget_limit: Some(50.0),
        ..TeamConstraints::default()
    };
    let ceiling = TeamConstraints {
        budget_limit: None,
        ..TeamConstraints::default()
    };
    let capped = validate_child_team_config(&resolved, &ceiling);
    assert_eq!(capped.budget_limit, Some(50.0));
}

#[test]
fn test_validate_child_team_config_caps_budget_none_vs_some() {
    // If resolved has no budget but ceiling does, use None (no budget is more restrictive)
    let resolved = TeamConstraints {
        budget_limit: None,
        ..TeamConstraints::default()
    };
    let ceiling = TeamConstraints {
        budget_limit: Some(30.0),
        ..TeamConstraints::default()
    };
    let capped = validate_child_team_config(&resolved, &ceiling);
    assert_eq!(capped.budget_limit, None);
}

#[test]
fn test_validate_child_team_config_empty_ceiling_tools_passes_resolved() {
    // Empty ceiling tools = no restriction, pass through resolved
    let resolved = TeamConstraints {
        allowed_tools: vec!["Read".to_string(), "Write".to_string()],
        ..TeamConstraints::default()
    };
    let ceiling = TeamConstraints {
        allowed_tools: vec![],
        ..TeamConstraints::default()
    };
    let capped = validate_child_team_config(&resolved, &ceiling);
    assert_eq!(capped.allowed_tools, vec!["Read", "Write"]);
}

#[test]
fn test_validate_child_team_config_empty_resolved_tools_gets_empty() {
    // Empty resolved tools = nothing to intersect
    let resolved = TeamConstraints {
        allowed_tools: vec![],
        ..TeamConstraints::default()
    };
    let ceiling = TeamConstraints {
        allowed_tools: vec!["Read".to_string()],
        ..TeamConstraints::default()
    };
    let capped = validate_child_team_config(&resolved, &ceiling);
    assert!(capped.allowed_tools.is_empty());
}

#[test]
fn test_validate_child_team_config_all_fields_capped() {
    let resolved = TeamConstraints {
        max_teammates: 10,
        allowed_tools: vec!["Read".to_string(), "Write".to_string(), "Edit".to_string()],
        allowed_mcp_tools: vec!["get_task_context".to_string(), "start_step".to_string()],
        model_cap: "opus".to_string(),
        mode: TeamMode::Dynamic,
        presets: vec!["ralphx-execution-coder".to_string(), "ralphx-execution-reviewer".to_string()],
        timeout_minutes: 60,
        budget_limit: Some(100.0),
        auto_approve: Some(true),
    };
    let ceiling = TeamConstraints {
        max_teammates: 3,
        allowed_tools: vec!["Read".to_string(), "Write".to_string()],
        allowed_mcp_tools: vec!["get_task_context".to_string()],
        model_cap: "sonnet".to_string(),
        mode: TeamMode::Constrained,
        presets: vec!["ralphx-execution-coder".to_string()],
        timeout_minutes: 30,
        budget_limit: Some(25.0),
        auto_approve: Some(false),
    };
    let capped = validate_child_team_config(&resolved, &ceiling);

    assert_eq!(capped.max_teammates, 3);
    assert_eq!(capped.allowed_tools, vec!["Read", "Write"]);
    assert_eq!(capped.allowed_mcp_tools, vec!["get_task_context"]);
    assert_eq!(capped.model_cap, "sonnet");
    // mode is NOT capped - it's inherited from resolved
    assert_eq!(capped.mode, TeamMode::Dynamic);
    assert_eq!(capped.presets, vec!["ralphx-execution-coder"]);
    assert_eq!(capped.timeout_minutes, 30);
    assert_eq!(capped.budget_limit, Some(25.0));
    // auto_approve is inherited from ceiling (parent controls)
    assert_eq!(capped.auto_approve, Some(false));
}

#[test]
fn test_validate_child_team_config_equal_constraints_unchanged() {
    let resolved = TeamConstraints {
        max_teammates: 5,
        allowed_tools: vec!["Read".to_string()],
        model_cap: "sonnet".to_string(),
        timeout_minutes: 30,
        budget_limit: Some(50.0),
        ..TeamConstraints::default()
    };
    let ceiling = resolved.clone();
    let capped = validate_child_team_config(&resolved, &ceiling);

    assert_eq!(capped.max_teammates, 5);
    assert_eq!(capped.allowed_tools, vec!["Read"]);
    assert_eq!(capped.model_cap, "sonnet");
    assert_eq!(capped.timeout_minutes, 30);
    assert_eq!(capped.budget_limit, Some(50.0));
}

// ── Integration: full YAML with process_mapping + team_constraints ──

#[test]
fn test_full_yaml_roundtrip() {
    let yaml = r#"
process_mapping:
  ideation:
    default: ralphx-ideation
    readonly: ralphx-ideation-readonly
    team: ralphx-ideation-team-lead
  execution:
    default: ralphx-execution-worker
    team: ralphx-execution-team-lead
  merge:
    default: ralphx-execution-merger

team_constraints:
  _defaults:
    max_teammates: 5
    model_cap: sonnet
    mode: dynamic
    timeout_minutes: 20
  execution:
    max_teammates: 5
    allowed_tools: [Read, Write, Edit, Bash]
    allowed_mcp_tools: [get_task_context, start_step]
    model_cap: sonnet
    mode: dynamic
    presets: [ralphx-execution-coder]
    timeout_minutes: 30
  review:
    max_teammates: 2
    mode: constrained
    presets: [ralphx-execution-reviewer]
"#;

    #[derive(Debug, Deserialize)]
    struct TestConfig {
        #[serde(default)]
        process_mapping: ProcessMapping,
        #[serde(default)]
        team_constraints: TeamConstraintsConfig,
    }

    let config: TestConfig = serde_yaml::from_str(yaml).unwrap();

    // Process mapping
    assert_eq!(config.process_mapping.slots.len(), 3);
    let ideation = &config.process_mapping.slots["ideation"];
    assert_eq!(ideation.default, "ralphx-ideation");
    assert_eq!(ideation.variants.get("team").unwrap(), "ralphx-ideation-team-lead");
    assert_eq!(
        ideation.variants.get("readonly").unwrap(),
        "ralphx-ideation-readonly"
    );

    // Team constraints
    let defaults = config.team_constraints.defaults.as_ref().unwrap();
    assert_eq!(defaults.max_teammates, 5);
    assert_eq!(defaults.mode, TeamMode::Dynamic);

    let exec = &config.team_constraints.processes["execution"];
    assert_eq!(exec.timeout_minutes, 30);
    assert_eq!(exec.allowed_tools.len(), 4);

    let review = &config.team_constraints.processes["review"];
    assert_eq!(review.mode, TeamMode::Constrained);
    assert_eq!(review.max_teammates, 2);

    // Test constraint resolution
    let exec_constraints = get_team_constraints(&config.team_constraints, "execution");
    assert_eq!(exec_constraints.max_teammates, 5);
    assert_eq!(exec_constraints.timeout_minutes, 30);

    // Ideation falls back to _defaults
    let ideation_constraints = get_team_constraints(&config.team_constraints, "ideation");
    assert_eq!(ideation_constraints.max_teammates, 5);
    assert_eq!(ideation_constraints.timeout_minutes, 20);

    // Process agent resolution
    assert_eq!(
        resolve_process_agent(&config.process_mapping, "execution", "team"),
        Some("ralphx-execution-team-lead".to_string())
    );
    assert_eq!(
        resolve_process_agent(&config.process_mapping, "execution", "default"),
        Some("ralphx-execution-worker".to_string())
    );
}

// ── auto_approve field tests ─────────────────────────────────────

#[test]
fn test_auto_approve_default_is_none() {
    let tc = TeamConstraints::default();
    assert!(tc.auto_approve.is_none());
}

#[test]
fn test_auto_approve_merge_defaults_false_no_override_gives_false() {
    // _defaults has auto_approve=false, no process-specific override → false
    let defaults = TeamConstraints {
        auto_approve: Some(false),
        ..TeamConstraints::default()
    };
    let specific = TeamConstraints {
        auto_approve: None,
        ..TeamConstraints::default()
    };
    let merged = merge_constraints(&defaults, &specific);
    assert_eq!(merged.auto_approve, Some(false));
}

#[test]
fn test_auto_approve_merge_both_none_defaults_to_true() {
    // Neither specific nor defaults has auto_approve → unwrap_or(true)
    let defaults = TeamConstraints {
        auto_approve: None,
        ..TeamConstraints::default()
    };
    let specific = TeamConstraints {
        auto_approve: None,
        ..TeamConstraints::default()
    };
    let merged = merge_constraints(&defaults, &specific);
    assert_eq!(merged.auto_approve, Some(true));
}

#[test]
fn test_auto_approve_merge_specific_overrides_defaults() {
    // _defaults=false, specific=true → specific wins
    let defaults = TeamConstraints {
        auto_approve: Some(false),
        ..TeamConstraints::default()
    };
    let specific = TeamConstraints {
        auto_approve: Some(true),
        ..TeamConstraints::default()
    };
    let merged = merge_constraints(&defaults, &specific);
    assert_eq!(merged.auto_approve, Some(true));
}

#[test]
fn test_auto_approve_yaml_deserialization_true() {
    let yaml = "auto_approve: true\n";
    let tc: TeamConstraints = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tc.auto_approve, Some(true));
}

#[test]
fn test_auto_approve_yaml_deserialization_false() {
    let yaml = "auto_approve: false\n";
    let tc: TeamConstraints = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(tc.auto_approve, Some(false));
}

#[test]
fn test_auto_approve_yaml_absent_gives_none() {
    let yaml = "max_teammates: 3\n";
    let tc: TeamConstraints = serde_yaml::from_str(yaml).unwrap();
    assert!(tc.auto_approve.is_none());
}

#[test]
fn test_auto_approve_child_inherits_ceiling() {
    let resolved = TeamConstraints {
        auto_approve: Some(true),
        ..TeamConstraints::default()
    };
    let ceiling = TeamConstraints {
        auto_approve: Some(false),
        ..TeamConstraints::default()
    };
    let capped = validate_child_team_config(&resolved, &ceiling);
    assert_eq!(capped.auto_approve, Some(false));
}

#[test]
fn test_auto_approve_child_ceiling_none_gives_none() {
    let resolved = TeamConstraints {
        auto_approve: Some(true),
        ..TeamConstraints::default()
    };
    let ceiling = TeamConstraints {
        auto_approve: None,
        ..TeamConstraints::default()
    };
    let capped = validate_child_team_config(&resolved, &ceiling);
    assert_eq!(capped.auto_approve, None);
}

// ── Test helpers ────────────────────────────────────────────────

fn default_spawn_request() -> TeammateSpawnRequest {
    TeammateSpawnRequest {
        role: String::new(),
        prompt: None,
        preset: None,
        tools: Vec::new(),
        mcp_tools: Vec::new(),
        model: "sonnet".to_string(),
        prompt_summary: None,
    }
}

fn build_test_mapping() -> ProcessMapping {
    let mut slots = HashMap::new();
    slots.insert(
        "execution".to_string(),
        ProcessSlot {
            default: "ralphx-execution-worker".to_string(),
            variants: {
                let mut v = HashMap::new();
                v.insert("team".to_string(), "ralphx-execution-team-lead".to_string());
                v
            },
        },
    );
    slots.insert(
        "ideation".to_string(),
        ProcessSlot {
            default: "ralphx-ideation".to_string(),
            variants: {
                let mut v = HashMap::new();
                v.insert("team".to_string(), "ralphx-ideation-team-lead".to_string());
                v.insert(
                    "readonly".to_string(),
                    "ralphx-ideation-readonly".to_string(),
                );
                v
            },
        },
    );
    ProcessMapping { slots }
}
