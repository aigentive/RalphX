use super::internal_skills::*;
use crate::domain::services::learned_skill_adapters::{
    LearnedSkillBucket, LearnedSkillConstraintCitation, LearnedSkillMultiSelectionRequest,
    LearnedSkillRecord, LearnedSkillStage, LearnedSkillStatus,
};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn create_agent(root: &Path, agent_yaml: &str) {
    fs::create_dir_all(root.join("agents/test-agent")).expect("create agent dir");
    fs::write(root.join("agents/test-agent/agent.yaml"), agent_yaml).expect("write agent");
}

fn create_skill(root: &Path, name: &str, body: &str) {
    fs::create_dir_all(root.join(format!("plugins/app/skills/{name}"))).expect("create skill dir");
    fs::write(
        root.join(format!("plugins/app/skills/{name}/SKILL.md")),
        body,
    )
    .expect("write skill");
}

#[test]
fn explicit_internal_directive_injects_allowlisted_internal_skill() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    create_agent(
        root,
        r#"name: test-agent
role: test
capabilities:
  internal_skills:
    allowed:
      - workspace-swe
"#,
    );
    create_skill(
        root,
        "workspace-swe",
        r#"---
name: workspace-swe
description: Workspace bridge instructions
disable-model-invocation: true
user-invocable: false
---
# Workspace SWE
Report only unless the event payload explicitly asks for intervention.
"#,
    );

    let injected = inject_internal_skills_into_system_prompt(
        root,
        "test-agent",
        "Base prompt",
        "Use /workspace-swe skill for this bridge wake-up.",
    )
    .expect("inject");

    assert_eq!(injected.injected_skill_names, vec!["workspace-swe"]);
    assert!(injected.system_prompt.contains("Base prompt"));
    assert!(injected.system_prompt.contains("# Workspace SWE"));
}

#[test]
fn disallowed_manual_skill_request_is_not_injected() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    create_agent(
        root,
        r#"name: test-agent
role: test
capabilities:
  internal_skills:
    allowed: []
"#,
    );
    create_skill(
        root,
        "workspace-swe",
        r#"---
name: workspace-swe
description: Workspace bridge instructions
---
# Workspace SWE
This should not load.
"#,
    );

    let injected = inject_internal_skills_into_system_prompt(
        root,
        "test-agent",
        "Base prompt",
        "Please use /workspace-swe.",
    )
    .expect("inject");

    assert!(injected.injected_skill_names.is_empty());
    assert!(!injected.system_prompt.contains("This should not load"));
}

#[test]
fn disabled_skill_does_not_auto_match_but_can_be_directed() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    create_agent(
        root,
        r#"name: test-agent
role: test
capabilities:
  internal_skills:
    auto_match: true
    allowed:
      - workspace-swe
"#,
    );
    create_skill(
        root,
        "workspace-swe",
        r#"---
name: workspace-swe
description: Workspace bridge instructions
trigger: workspace bridge
disable-model-invocation: true
user-invocable: false
---
# Workspace SWE
Forced only.
"#,
    );

    let auto = inject_internal_skills_into_system_prompt(
        root,
        "test-agent",
        "Base prompt",
        "workspace bridge",
    )
    .expect("auto inject");
    assert!(auto.injected_skill_names.is_empty());

    let directed = inject_internal_skills_into_system_prompt(
        root,
        "test-agent",
        "Base prompt",
        "<!-- ralphx_internal_skill=workspace-swe -->",
    )
    .expect("directed inject");
    assert_eq!(directed.injected_skill_names, vec!["workspace-swe"]);
    assert!(directed.system_prompt.contains("Forced only."));
}

#[test]
fn validation_rejects_unknown_allowed_skill() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    create_agent(
        root,
        r#"name: test-agent
role: test
capabilities:
  internal_skills:
    allowed:
      - missing-skill
"#,
    );

    let error = validate_agent_internal_skills(root, "test-agent")
        .expect_err("unknown skill should fail validation");
    assert!(error.contains("missing-skill"));
}

#[test]
fn directive_for_disallowed_skill_fails_closed() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    create_agent(
        root,
        r#"name: test-agent
role: test
capabilities:
  internal_skills:
    allowed:
      - workspace-swe
"#,
    );
    create_skill(
        root,
        "workspace-swe",
        r#"---
name: workspace-swe
description: Workspace bridge instructions
---
# Workspace SWE
"#,
    );

    let error = inject_internal_skills_into_system_prompt(
        root,
        "test-agent",
        "Base prompt",
        "<!-- ralphx_internal_skill=other-skill -->",
    )
    .expect_err("disallowed directive should fail closed");
    assert!(error.contains("other-skill"));
    assert!(error.contains("allowed"));
}

#[test]
fn live_agent_internal_skill_configs_are_valid() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    for agent_name in [
        "ralphx-chat-project",
        "ralphx-ideation",
        "ralphx-memory-capture",
        "ralphx-memory-maintainer",
    ] {
        validate_agent_internal_skills(&root, agent_name)
            .unwrap_or_else(|error| panic!("{agent_name} internal skills invalid: {error}"));
    }
}

#[test]
fn memory_agents_auto_load_project_skill_authoring_contract() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    for agent_name in ["ralphx-memory-capture", "ralphx-memory-maintainer"] {
        let injected = inject_internal_skills_into_system_prompt(
            &root,
            agent_name,
            "Base prompt",
            "Distill a learned project skill candidate from this conversation.",
        )
        .unwrap_or_else(|error| panic!("{agent_name} injection failed: {error}"));
        assert_eq!(
            injected.injected_skill_names,
            vec!["project-skill-authoring"],
            "{agent_name} should load the authoring contract"
        );
        assert!(injected.system_prompt.contains("# Project Skill Authoring"));
        assert!(injected
            .system_prompt
            .contains("Do not create one skill per commit, PR, error string, or session."));
    }
}

#[test]
fn live_general_workspace_agents_do_not_load_workspace_bridge_skill() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let directive = "<!-- ralphx_internal_skill=ralphx-agent-workspace-swe -->";

    let worker_error = inject_internal_skills_into_system_prompt(
        &root,
        "ralphx-general-worker",
        "Base prompt",
        directive,
    )
    .expect_err("worker must reject a disallowed workspace bridge directive");
    assert!(worker_error.contains("ralphx-agent-workspace-swe"));
    assert!(worker_error.contains("not listed in allowed"));

    let explorer = inject_internal_skills_into_system_prompt(
        &root,
        "ralphx-general-explorer",
        "Base prompt",
        directive,
    )
    .expect("explorer without internal skills should leave the prompt unchanged");
    assert!(explorer.injected_skill_names.is_empty());
    assert_eq!(explorer.system_prompt, "Base prompt");
}

#[test]
fn list_internal_skill_summaries_sorts_and_preserves_invocability() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    create_agent(
        root,
        r#"name: test-agent
role: test
capabilities:
  internal_skills:
    allowed:
      - zebra-skill
      - alpha-skill
"#,
    );
    create_skill(
        root,
        "zebra-skill",
        r#"---
name: zebra-skill
description: Last alphabetically
user-invocable: false
---
Zebra body.
"#,
    );
    create_skill(
        root,
        "alpha-skill",
        r#"---
name: alpha-skill
description: First alphabetically
---
Alpha body.
"#,
    );

    let summaries = list_internal_skill_summaries_for_agent(root, "test-agent").expect("summaries");

    assert_eq!(
        summaries
            .iter()
            .map(|summary| summary.name.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha-skill", "zebra-skill"]
    );
    assert_eq!(
        summaries[0].description.as_deref(),
        Some("First alphabetically")
    );
    assert!(summaries[0].user_invocable);
    assert!(!summaries[1].user_invocable);
    assert!(summaries[0]
        .source_path
        .ends_with("plugins/app/skills/alpha-skill/SKILL.md"));
}

#[test]
fn auto_match_respects_priority_limit_and_description_hits() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    create_agent(
        root,
        r#"name: test-agent
role: test
capabilities:
  internal_skills:
    auto_match: true
    max_auto_loaded: 1
    allowed:
      - lower-priority
      - higher-priority
"#,
    );
    create_skill(
        root,
        "lower-priority",
        r#"---
name: lower-priority
description: Refactor project composer workflows
trigger: composer workflow
priority: 0
---
Lower body.
"#,
    );
    create_skill(
        root,
        "higher-priority",
        r#"---
name: higher-priority
description: Refactor project composer workflows
trigger: composer workflow
priority: 10
---
Higher body.
"#,
    );

    let injected = inject_internal_skills_into_system_prompt(
        root,
        "test-agent",
        "Base prompt",
        "Please refactor the project composer workflow.",
    )
    .expect("inject");

    assert_eq!(injected.injected_skill_names, vec!["higher-priority"]);
    assert!(injected.system_prompt.contains("Higher body."));
    assert!(!injected.system_prompt.contains("Lower body."));
}

#[test]
fn invalid_internal_skill_files_fail_with_precise_errors() {
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    create_skill(root, "bad-frontmatter", "Not frontmatter");
    let frontmatter_error =
        load_internal_skill(root, "bad-frontmatter").expect_err("frontmatter required");
    assert!(frontmatter_error.contains("must start with YAML frontmatter"));

    create_skill(
        root,
        "wrong-name",
        r#"---
name: other-name
---
Body.
"#,
    );
    let name_error = load_internal_skill(root, "wrong-name").expect_err("name mismatch");
    assert!(name_error.contains("declares mismatched name"));
}

#[test]
fn directive_extraction_accepts_legacy_and_use_skill_forms_once() {
    let directives = extract_internal_skill_directives(
        "Use /workspace-swe skill now.\n<!-- ralphx_internal_skill=workspace-swe -->",
    );

    assert_eq!(directives, vec!["workspace-swe"]);
    assert!(is_manual_invocation(
        "Please run /workspace-swe.",
        "workspace-swe"
    ));
    assert_eq!(
        split_match_terms("Workspace bridge, code-quality."),
        vec!["workspace", "bridge", "code-quality"]
    );
}

fn learned_skill_record(id: &str) -> LearnedSkillRecord {
    LearnedSkillRecord {
        id: id.to_string(),
        project_id: "project-1".to_string(),
        title: format!("Skill {id}"),
        status: LearnedSkillStatus::Approved,
        pinned: false,
        caller_surfaces: vec!["reviewer".to_string()],
        stages: vec![LearnedSkillStage::Review],
        buckets: vec![LearnedSkillBucket::Review],
        path_scopes: Vec::new(),
        compact_guidance: "Use this when reviewing repeated failures.".to_string(),
        predicted_effect: "Reduces repeated review mistakes.".to_string(),
        provenance_refs: vec!["pr-42".to_string()],
    }
}

#[test]
fn learned_skill_citations_escape_and_filter_unsafe_ids() {
    let citations = vec![
        LearnedSkillConstraintCitation {
            skill_id: "unsafe/id".to_string(),
            title: "Unsafe".to_string(),
            predicted_effect: "Should not appear".to_string(),
            compact_guidance: "Skip".to_string(),
            provenance_refs: Vec::new(),
        },
        LearnedSkillConstraintCitation {
            skill_id: "skill-1".to_string(),
            title: "Review <merge> & \"quotes\"".to_string(),
            predicted_effect: "Avoid <bad> output".to_string(),
            compact_guidance: "Use 'carefully'".to_string(),
            provenance_refs: vec!["pr<42>".to_string()],
        },
    ];

    let injected = inject_learned_skill_citations_into_system_prompt("Base", &citations);

    assert_eq!(injected.injected_skill_names, vec!["learned:skill-1"]);
    assert!(injected
        .system_prompt
        .contains("Review &lt;merge&gt; &amp; &quot;quotes&quot;"));
    assert!(injected.system_prompt.contains("Use &#39;carefully&#39;"));
    assert!(injected.system_prompt.contains("pr&lt;42&gt;"));
    assert!(!injected.system_prompt.contains("Should not appear"));
}

#[test]
fn learned_skill_injection_extends_existing_injection_only_when_selected() {
    let base = InternalSkillInjection {
        system_prompt: "Base prompt".to_string(),
        injected_skill_names: vec!["workspace-swe".to_string()],
    };
    let no_context =
        inject_pre_execution_learned_skills_into_existing_injection(base.clone(), None);
    assert_eq!(no_context, base);

    let context = PreExecutionLearnedSkillContext {
        request: LearnedSkillMultiSelectionRequest {
            project_id: "project-1".to_string(),
            caller_surface: "reviewer".to_string(),
            stages: vec![LearnedSkillStage::Review],
            buckets: vec![LearnedSkillBucket::Review],
            touched_paths: Vec::new(),
            max_skills: 1,
        },
        available_skills: vec![learned_skill_record("skill-1")],
        max_total_chars: 6_000,
        max_guidance_chars: 400,
    };

    let injected =
        inject_pre_execution_learned_skills_into_existing_injection(base, Some(&context));

    assert_eq!(
        injected.injected_skill_names,
        vec!["workspace-swe", "learned:skill-1"]
    );
    assert!(injected
        .system_prompt
        .contains("<ralphx_learned_skill_citations>"));
}

#[test]
fn c1_learned_skill_writer_enforces_unicode_guidance_and_entry_budgets() {
    let citations = (0..5)
        .map(|index| LearnedSkillConstraintCitation {
            skill_id: format!("skill-{index}"),
            title: format!("Skill {index}"),
            predicted_effect: "Reduce repeated failures.".to_string(),
            compact_guidance: "🦀".repeat(401),
            provenance_refs: vec![format!("outcome-{index}")],
        })
        .collect::<Vec<_>>();

    let injected = inject_bounded_learned_skill_citations_into_system_prompt(
        "Base",
        &citations,
        usize::MAX,
        usize::MAX,
        usize::MAX,
    );

    assert_eq!(injected.injected_skill_names.len(), 4);
    assert!(!injected.system_prompt.contains("skill-4"));
    let first_guidance = injected
        .system_prompt
        .lines()
        .find_map(|line| line.strip_prefix("guidance: "))
        .expect("guidance line");
    assert_eq!(first_guidance.chars().count(), 400);
    assert_eq!(
        injected
            .system_prompt
            .matches("<learned_skill_citation ")
            .count(),
        4
    );
}

#[test]
fn c1_learned_skill_writer_drops_over_budget_citation_whole() {
    let citations = vec![
        LearnedSkillConstraintCitation {
            skill_id: "skill-small".to_string(),
            title: "Small".to_string(),
            predicted_effect: "Useful".to_string(),
            compact_guidance: "Keep this.".to_string(),
            provenance_refs: Vec::new(),
        },
        LearnedSkillConstraintCitation {
            skill_id: "skill-large".to_string(),
            title: "x".repeat(1_000),
            predicted_effect: "y".repeat(1_000),
            compact_guidance: "z".repeat(400),
            provenance_refs: Vec::new(),
        },
    ];

    let injected =
        inject_bounded_learned_skill_citations_into_system_prompt("Base", &citations, 4, 500, 400);

    assert_eq!(injected.injected_skill_names, vec!["learned:skill-small"]);
    assert!(injected.system_prompt.contains("skill-small"));
    assert!(!injected.system_prompt.contains("skill-large"));
    assert_eq!(
        injected
            .system_prompt
            .matches("</learned_skill_citation>")
            .count(),
        1
    );
    assert!(injected
        .system_prompt
        .ends_with("</ralphx_learned_skill_citations>"));
}
