use super::skills::*;
use std::collections::BTreeSet;
use std::path::Path;

use crate::domain::entities::{
    ProjectId, ProjectSkill, ProjectSkillLifecycleStatus,
};

    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn project_skill_composer_response_carries_stable_learned_identity() {
        let skill = ProjectSkill {
            id: crate::domain::entities::ProjectSkillId::from_string("skill-123"),
            project_id: ProjectId::from_string("project-1".to_string()),
            title: "Review Error Paths".to_string(),
            bucket: "reviewer".to_string(),
            stage: "approved".to_string(),
            status: ProjectSkillLifecycleStatus::Approved,
            pinned: false,
            archived: false,
            scope_paths: Vec::new(),
            compact_guidance: "Check fail-closed paths.".to_string(),
            body_markdown: "Check fail-closed paths.".to_string(),
            predicted_effect: Some("Reduces missed rejection paths.".to_string()),
            provenance_json: serde_json::json!({ "source": "test" }),
            companion_of_skill_id: None,
            content_hash: String::new(),
            evidence_hash: String::new(),
            created_by: crate::domain::entities::ProjectSkillCreatedBy::User,
            pipeline_role: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let response = project_skill_to_composer_skill(skill);

        assert_eq!(response.id, "learned:skill-123");
        assert_eq!(response.source, "learned");
        assert_eq!(response.name, "review-error-paths");
        assert_eq!(response.display_name.as_deref(), Some("Review Error Paths"));
        assert_eq!(response.invocation_kind, "project-skill-directive");
        assert_eq!(response.invocation_value, "skill-123");
        // Composer surfaces the open-standard compact guidance (what+when),
        // matching the exported SKILL.md description.
        assert_eq!(
            response.description.as_deref(),
            Some("Check fail-closed paths.")
        );
    }

    #[test]
    fn project_skill_composer_response_falls_back_to_predicted_effect_and_safe_id_token() {
        let skill = ProjectSkill {
            id: crate::domain::entities::ProjectSkillId::from_string("skill.id:unsafe_123"),
            project_id: ProjectId::from_string("project-1".to_string()),
            title: "!!!".to_string(),
            bucket: "reviewer".to_string(),
            stage: "approved".to_string(),
            status: ProjectSkillLifecycleStatus::Approved,
            pinned: false,
            archived: false,
            scope_paths: Vec::new(),
            compact_guidance: "   ".to_string(),
            body_markdown: "Check fail-closed paths.".to_string(),
            predicted_effect: Some("  Reduces repeated mistakes.  ".to_string()),
            provenance_json: serde_json::json!({ "source": "test" }),
            companion_of_skill_id: None,
            content_hash: String::new(),
            evidence_hash: String::new(),
            created_by: crate::domain::entities::ProjectSkillCreatedBy::User,
            pipeline_role: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let response = project_skill_to_composer_skill(skill);

        assert_eq!(response.name, "skillidunsafe123");
        assert_eq!(
            response.description.as_deref(),
            Some("Reduces repeated mistakes.")
        );
    }

    #[test]
    fn claude_native_skill_discovery_reads_project_skills_and_commands() {
        let temp = tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join(".claude/skills/ship-it")).expect("skill dir");
        fs::write(
            temp.path().join(".claude/skills/ship-it/SKILL.md"),
            r#"---
name: ship-it
description: Ship focused changes
---
# Ship It
"#,
        )
        .expect("skill");
        fs::create_dir_all(temp.path().join(".claude/commands")).expect("commands dir");
        fs::write(
            temp.path().join(".claude/commands/review.md"),
            "Review the current work.",
        )
        .expect("command");

        let skills = list_claude_native_skills(temp.path(), "missing-agent").expect("skills");

        assert!(skills.iter().any(|skill| {
            skill.id == "claude:project:ship-it"
                && skill.provider_harness.as_deref() == Some("claude")
        }));
        assert!(skills
            .iter()
            .any(|skill| skill.id == "claude:project-command:review"));
    }

    #[test]
    fn codex_native_skill_discovery_reads_repo_and_plugin_skills() {
        let temp = tempdir().expect("tempdir");
        let disabled_paths = BTreeSet::new();
        fs::create_dir_all(temp.path().join(".agents/skills/ship-it")).expect("skill dir");
        fs::write(
            temp.path().join(".agents/skills/ship-it/SKILL.md"),
            r#"---
name: ship-it
description: Ship focused changes
---
# Ship It
"#,
        )
        .expect("repo skill");
        let repo_skills = read_codex_skill_dir(
            &temp.path().join(".agents/skills"),
            "repo",
            None,
            &disabled_paths,
        )
        .expect("repo skills");

        assert!(repo_skills.iter().any(|skill| {
            skill.id == "codex:repo:ship-it"
                && skill.name == "ship-it"
                && skill.invocation_value == "$ship-it"
        }));

        fs::create_dir_all(temp.path().join("plugins/github/skills/yeet")).expect("plugin skill");
        fs::create_dir_all(temp.path().join("plugins/github/.codex-plugin"))
            .expect("plugin metadata");
        fs::write(
            temp.path().join("plugins/github/.codex-plugin/plugin.json"),
            r#"{"name":"github","skills":"./skills/"}"#,
        )
        .expect("plugin manifest");
        fs::write(
            temp.path().join("plugins/github/skills/yeet/SKILL.md"),
            r#"---
name: yeet
description: Publish through GitHub
---
# Yeet
"#,
        )
        .expect("plugin skill");
        let (plugin_name, plugin_skills_dir) =
            read_codex_plugin_skill_root(&temp.path().join("plugins/github")).expect("plugin root");
        let plugin_skills = read_codex_skill_dir(
            &plugin_skills_dir,
            "plugin",
            Some(&plugin_name),
            &disabled_paths,
        )
        .expect("plugin skills");

        assert!(plugin_skills.iter().any(|skill| {
            skill.id == "codex:plugin:github:yeet"
                && skill.name == "github:yeet"
                && skill.invocation_value == "$github:yeet"
        }));
    }

    #[test]
    fn codex_disabled_skill_config_marks_canonical_skill_path_disabled() {
        let temp = tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("skills/local")).expect("skill dir");
        let skill_file = temp.path().join("skills/local/SKILL.md");
        fs::write(
            &skill_file,
            r#"---
name: local
description: Local skill
---
Body
"#,
        )
        .expect("skill");
        let config = format!(
            r#"
[[skills.config]]
path = "{}"
enabled = false
"#,
            skill_file.display()
        );
        let disabled_paths = parse_disabled_codex_skill_paths(&config);
        let skills =
            read_codex_skill_dir(&temp.path().join("skills"), "user", None, &disabled_paths)
                .expect("skills");

        assert_eq!(skills.len(), 1);
        assert!(!skills[0].enabled);
    }

    fn create_agent(root: &Path, agent_yaml: &str) {
        fs::create_dir_all(root.join("agents/test-agent")).expect("agent dir");
        fs::write(root.join("agents/test-agent/agent.yaml"), agent_yaml).expect("agent yaml");
    }

    fn create_internal_skill(root: &Path, name: &str, frontmatter: &str, body: &str) {
        fs::create_dir_all(root.join(format!("plugins/app/skills/{name}"))).expect("skill dir");
        fs::write(
            root.join(format!("plugins/app/skills/{name}/SKILL.md")),
            format!("---\n{frontmatter}\n---\n{body}\n"),
        )
        .expect("skill file");
    }

    #[test]
    fn internal_composer_skills_exposes_only_user_invocable_allowed_skills() {
        let temp = tempdir().expect("tempdir");
        create_agent(
            temp.path(),
            r#"name: test-agent
role: test
capabilities:
  internal_skills:
    allowed:
      - quick-fix
      - hidden-bridge
"#,
        );
        create_internal_skill(
            temp.path(),
            "quick-fix",
            "name: quick-fix\ndescription: Apply focused fixes",
            "Quick fix instructions.",
        );
        create_internal_skill(
            temp.path(),
            "hidden-bridge",
            "name: hidden-bridge\nuser-invocable: false",
            "Hidden bridge instructions.",
        );

        let skills =
            list_internal_composer_skills(temp.path(), "test-agent").expect("internal skills");

        assert_eq!(skills.len(), 1);
        let skill = &skills[0];
        assert_eq!(skill.id, "internal:quick-fix");
        assert_eq!(skill.name, "quick-fix");
        assert_eq!(skill.description.as_deref(), Some("Apply focused fixes"));
        assert_eq!(skill.source, "ralphx-internal");
        assert_eq!(skill.invocation_kind, "internal-directive");
        assert_eq!(skill.invocation_value, "quick-fix");
        assert!(skill.enabled);
        assert!(skill
            .source_path
            .as_deref()
            .is_some_and(|path| path.ends_with("plugins/app/skills/quick-fix/SKILL.md")));
    }

    #[test]
    fn claude_skill_reader_uses_metadata_fallbacks_and_filters_commands() {
        let temp = tempdir().expect("tempdir");
        let skill_root = temp.path().join(".claude/skills");
        fs::create_dir_all(skill_root.join("fallback-skill")).expect("skill dir");
        fs::write(
            skill_root.join("fallback-skill/SKILL.md"),
            r#"---
name: ../unsafe
display-name: Fallback Skill
when_to_use: Use when metadata name is unsafe
user-invocable: false
---
Skill body.
"#,
        )
        .expect("skill file");

        let skills = read_claude_skill_dir(&skill_root, "project", None).expect("skills");
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "claude:project:fallback-skill");
        assert_eq!(skills[0].name, "fallback-skill");
        assert_eq!(skills[0].display_name.as_deref(), Some("Fallback Skill"));
        assert_eq!(
            skills[0].description.as_deref(),
            Some("Use when metadata name is unsafe")
        );
        assert!(!skills[0].enabled);

        let commands_root = temp.path().join(".claude/commands");
        fs::create_dir_all(&commands_root).expect("commands dir");
        fs::write(
            commands_root.join("review.md"),
            "\nReview the current branch.\n",
        )
        .expect("command file");
        fs::write(commands_root.join("bad.name.md"), "Should be ignored.")
            .expect("unsafe command file");
        fs::write(commands_root.join("notes.txt"), "Should also be ignored.")
            .expect("non-command file");

        let commands =
            read_claude_skill_dir(&commands_root, "project-command", None).expect("commands");

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].id, "claude:project-command:review");
        assert_eq!(
            commands[0].description.as_deref(),
            Some("Review the current branch.")
        );
        assert_eq!(commands[0].invocation_value, "/review");
    }

    #[test]
    fn claude_command_reader_prefers_frontmatter_description() {
        let temp = tempdir().expect("tempdir");
        let commands_root = temp.path().join(".claude/commands");
        fs::create_dir_all(&commands_root).expect("commands dir");
        fs::write(
            commands_root.join("ship.md"),
            r#"---
description: Prefer metadata
display-name: Ship Command
user-invocable: false
---
Fallback first body line.
"#,
        )
        .expect("command file");

        let commands =
            read_claude_skill_dir(&commands_root, "project-command", None).expect("commands");

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].display_name.as_deref(), Some("Ship Command"));
        assert_eq!(commands[0].description.as_deref(), Some("Prefer metadata"));
        assert!(!commands[0].enabled);
    }

    #[test]
    fn claude_plugin_roots_use_enabled_installed_plugins_and_namespace_invocations() {
        let temp = tempdir().expect("tempdir");
        let claude_home = temp.path().join(".claude");
        let plugins_dir = claude_home.join("plugins");
        let active_root = plugins_dir.join("cache/claude-plugins-official/figma/2.2.50");
        let stale_root = plugins_dir.join("cache/claude-plugins-official/figma/2.1.30");
        let disabled_root = plugins_dir.join("cache/claude-plugins-official/microsoft-docs/0.3.1");

        fs::create_dir_all(active_root.join("skills/figma-use")).expect("active skill dir");
        fs::write(
            active_root.join("skills/figma-use/SKILL.md"),
            r#"---
name: figma-use
description: Use Figma
---
Body
"#,
        )
        .expect("active skill");
        fs::create_dir_all(active_root.join("commands")).expect("active commands dir");
        fs::write(
            active_root.join("commands/sync.md"),
            "Synchronize Figma assets.",
        )
        .expect("active command");
        fs::create_dir_all(stale_root.join("skills/old-only")).expect("stale skill dir");
        fs::write(
            stale_root.join("skills/old-only/SKILL.md"),
            "---\nname: old-only\n---\nBody\n",
        )
        .expect("stale skill");
        fs::create_dir_all(disabled_root.join("skills/microsoft-docs"))
            .expect("disabled skill dir");
        fs::write(
            disabled_root.join("skills/microsoft-docs/SKILL.md"),
            "---\nname: microsoft-docs\n---\nBody\n",
        )
        .expect("disabled skill");
        fs::write(
            claude_home.join(CLAUDE_SETTINGS_FILE_NAME),
            serde_json::json!({
                "enabledPlugins": {
                    "figma@claude-plugins-official": true,
                    "microsoft-docs@claude-plugins-official": false
                }
            })
            .to_string(),
        )
        .expect("settings");
        fs::write(
            plugins_dir.join(CLAUDE_INSTALLED_PLUGINS_FILE_NAME),
            serde_json::json!({
                "version": 2,
                "plugins": {
                    "figma@claude-plugins-official": [
                        { "scope": "user", "installPath": active_root }
                    ],
                    "microsoft-docs@claude-plugins-official": [
                        { "scope": "user", "installPath": disabled_root }
                    ]
                }
            })
            .to_string(),
        )
        .expect("installed plugins");

        let mut roots = Vec::new();
        let mut seen = BTreeSet::new();
        push_claude_plugin_roots(&mut roots, &mut seen, &claude_home);
        let mut skills = Vec::new();
        for root in roots {
            skills.extend(
                read_claude_skill_dir(&root.path, &root.scope, root.name_prefix.as_deref())
                    .expect("plugin skills"),
            );
        }

        assert!(skills.iter().any(|skill| {
            skill.id == "claude:plugin:figma:figma-use"
                && skill.name == "figma:figma-use"
                && skill.invocation_value == "/figma:figma-use"
                && skill
                    .source_path
                    .as_deref()
                    .is_some_and(|path| path.ends_with("figma/2.2.50/skills/figma-use/SKILL.md"))
        }));
        assert!(skills.iter().any(|skill| {
            skill.id == "claude:plugin-command:figma:sync"
                && skill.name == "figma:sync"
                && skill.invocation_value == "/figma:sync"
        }));
        assert!(!skills.iter().any(|skill| skill.name == "figma:old-only"));
        assert!(!skills
            .iter()
            .any(|skill| skill.name == "microsoft-docs:microsoft-docs"));
    }

    #[test]
    fn claude_canonical_agent_skill_entries_are_included() {
        let temp = tempdir().expect("tempdir");
        create_agent(
            temp.path(),
            r#"name: test-agent
role: test
harnesses:
  claude:
    skills:
      - ship-it
"#,
        );

        let skills = list_claude_native_skills(temp.path(), "test-agent").expect("skills");

        assert!(skills.iter().any(|skill| {
            skill.id == "claude:canonical:ship-it"
                && skill.invocation_value == "/ship-it"
                && skill.scope.as_deref() == Some("agent")
        }));
    }

    #[test]
    fn codex_skill_reader_uses_body_description_prefix_and_disabled_set() {
        let temp = tempdir().expect("tempdir");
        let skills_root = temp.path().join("skills");
        fs::create_dir_all(skills_root.join("local")).expect("skill dir");
        let skill_file = skills_root.join("local/SKILL.md");
        fs::write(
            &skill_file,
            r#"---
name: bad/name
display-name: Local Skill
---
First body line is the description.

More body.
"#,
        )
        .expect("skill file");
        let disabled_paths = BTreeSet::from([skill_file.canonicalize().expect("canonical")]);

        let skills = read_codex_skill_dir(&skills_root, "plugin", Some("github"), &disabled_paths)
            .expect("skills");

        assert_eq!(skills.len(), 1);
        let skill = &skills[0];
        assert_eq!(skill.id, "codex:plugin:github:local");
        assert_eq!(skill.name, "github:local");
        assert_eq!(skill.display_name.as_deref(), Some("Local Skill"));
        assert_eq!(
            skill.description.as_deref(),
            Some("First body line is the description.")
        );
        assert_eq!(skill.invocation_value, "$github:local");
        assert!(!skill.enabled);
    }

    #[test]
    fn codex_skill_reader_uses_frontmatter_description_and_ignores_missing_skill_files() {
        let temp = tempdir().expect("tempdir");
        let skills_root = temp.path().join("skills");
        fs::create_dir_all(skills_root.join("with-file")).expect("skill dir");
        fs::create_dir_all(skills_root.join("without-file")).expect("missing skill dir");
        fs::write(
            skills_root.join("with-file/SKILL.md"),
            r#"---
name: with-file
description: Metadata description
---
Body fallback.
"#,
        )
        .expect("skill file");

        let skills =
            read_codex_skill_dir(&skills_root, "repo", None, &BTreeSet::new()).expect("skills");

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "codex:repo:with-file");
        assert_eq!(
            skills[0].description.as_deref(),
            Some("Metadata description")
        );
        assert!(skills[0].enabled);
    }

    #[test]
    fn codex_config_parser_handles_quotes_comments_and_table_boundaries() {
        let temp = tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("skills/disabled")).expect("disabled skill dir");
        fs::create_dir_all(temp.path().join("skills/enabled")).expect("enabled skill dir");
        let disabled = temp.path().join("skills/disabled/SKILL.md");
        let enabled = temp.path().join("skills/enabled/SKILL.md");
        fs::write(&disabled, "---\nname: disabled\n---\nBody").expect("disabled skill");
        fs::write(&enabled, "---\nname: enabled\n---\nBody").expect("enabled skill");
        let config = format!(
            r#"
[[skills.config]]
path = '{}' # single quotes are accepted
enabled = false

[[skills.config]]
path = "{}"
enabled = true

[other]
path = "/tmp/ignored"
enabled = false
"#,
            disabled.display(),
            enabled.display()
        );

        let disabled_paths = parse_disabled_codex_skill_paths(&config);

        assert!(disabled_paths.contains(&disabled.canonicalize().expect("canonical disabled")));
        assert!(!disabled_paths.contains(&enabled.canonicalize().expect("canonical enabled")));
    }

    #[test]
    fn codex_plugin_roots_reject_unsafe_manifests_and_accept_cache_plugins() {
        let temp = tempdir().expect("tempdir");
        let unsafe_plugin = temp.path().join("unsafe-plugin");
        fs::create_dir_all(unsafe_plugin.join(".codex-plugin")).expect("unsafe metadata");
        fs::create_dir_all(temp.path().join("outside-skills")).expect("outside skills");
        fs::write(
            unsafe_plugin.join(".codex-plugin/plugin.json"),
            r#"{"name":"unsafe","skills":"../outside-skills"}"#,
        )
        .expect("unsafe manifest");

        assert!(read_codex_plugin_skill_root(&unsafe_plugin).is_none());

        let absolute_plugin = temp.path().join("absolute-plugin");
        fs::create_dir_all(absolute_plugin.join(".codex-plugin")).expect("absolute metadata");
        fs::write(
            absolute_plugin.join(".codex-plugin/plugin.json"),
            r#"{"name":"absolute","skills":"/tmp/skills"}"#,
        )
        .expect("absolute manifest");
        assert!(read_codex_plugin_skill_root(&absolute_plugin).is_none());

        let unsafe_name_plugin = temp.path().join("unsafe-name-plugin");
        fs::create_dir_all(unsafe_name_plugin.join(".codex-plugin")).expect("unsafe metadata");
        fs::write(
            unsafe_name_plugin.join(".codex-plugin/plugin.json"),
            r#"{"name":"bad/name","skills":"./skills"}"#,
        )
        .expect("unsafe name manifest");
        assert!(read_codex_plugin_skill_root(&unsafe_name_plugin).is_none());

        let plugin_root = temp
            .path()
            .join("plugins/cache/openai-curated/github/version-1");
        fs::create_dir_all(plugin_root.join(".codex-plugin")).expect("plugin metadata");
        fs::create_dir_all(plugin_root.join("skills/yeet")).expect("plugin skill");
        fs::write(
            plugin_root.join(".codex-plugin/plugin.json"),
            r#"{"name":"github","skills":"./skills"}"#,
        )
        .expect("plugin manifest");
        fs::write(
            plugin_root.join("skills/yeet/SKILL.md"),
            "---\nname: yeet\n---\nBody",
        )
        .expect("plugin skill file");

        let mut roots = Vec::new();
        let mut seen = BTreeSet::new();
        push_codex_plugin_roots(&mut roots, &mut seen, temp.path());

        assert!(roots.iter().any(|root| {
            root.scope == "plugin"
                && root.name_prefix.as_deref() == Some("github")
                && root
                    .path
                    .ends_with("plugins/cache/openai-curated/github/version-1/skills")
        }));
    }

    #[test]
    fn skill_helpers_ignore_invalid_inputs() {
        assert!(split_frontmatter("No frontmatter").is_none());
        assert!(is_safe_skill_token("safe-name_1"));
        assert!(!is_safe_skill_token("bad/name"));
        assert_eq!(
            parse_toml_key_value("enabled = false", "enabled"),
            Some("false")
        );
        assert_eq!(
            parse_toml_string("\"hello\" # ignored"),
            Some("hello".to_string())
        );
        assert_eq!(parse_toml_string("unquoted"), None);
    }
