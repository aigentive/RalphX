use super::validation::{
    compose_persona_content, compute_content_hash, reject_structural_tags,
    validate_persona_content, PERSONA_BODY_MAX_BYTES, PERSONA_BODY_MAX_LINES,
};

fn persona_content(slug: &str, body: &str) -> String {
    format!("---\nname: {slug}\nkind: persona\ndescription: Test persona\n---\n{body}")
}

#[test]
fn parse_persona_accepts_minimal_valid_skill_md() {
    let parsed = validate_persona_content("test-persona", &persona_content("test-persona", "Body"))
        .expect("minimal persona should be valid");

    assert_eq!(parsed.frontmatter.name, "test-persona");
    assert_eq!(parsed.frontmatter.kind.as_deref(), Some("persona"));
    assert_eq!(parsed.frontmatter.description, "Test persona");
    assert_eq!(parsed.body, "Body");

    let skill = "---\nname: test-persona\nkind: skill\ndescription: Test persona\n---\nBody";
    assert!(validate_persona_content("test-persona", skill).is_err());

    let missing_kind = "---\nname: test-persona\ndescription: Test persona\n---\nBody";
    assert!(validate_persona_content("test-persona", missing_kind).is_err());
}

#[test]
fn parse_persona_rejects_name_slug_mismatch() {
    let error = validate_persona_content("test-persona", &persona_content("other-persona", "Body"))
        .expect_err("mismatched names must fail");

    assert!(error.to_string().contains("test-persona"));
}

#[test]
fn persona_slug_rejects_untrusted_charset() {
    for slug in [
        "",
        "..",
        "bad/name",
        "bad\\name",
        "Uppercase",
        "under_score",
        "unicod\u{e9}",
    ] {
        assert!(validate_persona_content(slug, &persona_content(slug, "Body")).is_err());
    }

    assert!(
        validate_persona_content("normal-kebab-2", &persona_content("normal-kebab-2", "Body"))
            .is_ok()
    );
}

#[test]
fn persona_body_caps_enforced_at_boundary() {
    let max_bytes = "x".repeat(PERSONA_BODY_MAX_BYTES);
    assert!(
        validate_persona_content("test-persona", &persona_content("test-persona", &max_bytes))
            .is_ok()
    );
    let over_bytes = "x".repeat(PERSONA_BODY_MAX_BYTES + 1);
    assert!(validate_persona_content(
        "test-persona",
        &persona_content("test-persona", &over_bytes)
    )
    .is_err());

    let max_lines = vec!["x"; PERSONA_BODY_MAX_LINES].join("\n");
    assert!(
        validate_persona_content("test-persona", &persona_content("test-persona", &max_lines))
            .is_ok()
    );
    let over_lines = vec!["x"; PERSONA_BODY_MAX_LINES + 1].join("\n");
    assert!(validate_persona_content(
        "test-persona",
        &persona_content("test-persona", &over_lines)
    )
    .is_err());
}

#[test]
fn structural_tag_blocklist_rejects_all_variants() {
    let tags = [
        "ralphx_agent_persona",
        "persona_precedence",
        "ralphx_internal_skills",
        "internal_skill",
        "internal_skill_metadata",
        "agent_runtime_profile",
        "ralphx_agent_instructions",
        "agent_task_ledger_contract",
    ];

    for tag in tags {
        for candidate in [
            format!("<{tag}>"),
            format!("</{tag}>"),
            format!("< {tag}>"),
            format!("<\t{tag}>"),
            format!("<{}>", tag.to_ascii_uppercase()),
        ] {
            assert!(reject_structural_tags(&candidate).is_err(), "{candidate}");
        }
    }

    assert!(reject_structural_tags("This persona explains how to collaborate.").is_ok());
}

#[test]
fn structural_tag_scan_ignores_normal_angle_bracket_text_but_detects_whitespace_after_close() {
    assert!(reject_structural_tags("2 < 3; use <ordinary-example> in prose.").is_ok());

    let blocked = "A closing tag can still be structural: </\nralphx_agent_persona>";
    assert!(reject_structural_tags(blocked).is_err());
}

#[test]
fn content_hash_is_stable_and_input_sensitive() {
    let frontmatter = "name: test\nkind: persona\ndescription: Test\n";
    let body = "body";
    let hash = compute_content_hash(frontmatter, body);

    assert_eq!(
        hash,
        "fc5200f350f11794ec0ce37340c0b6cc98efae37bde18a5335acb38892ee19f9"
    );
    assert_ne!(hash, compute_content_hash(frontmatter, "body!"));
    assert_ne!(
        hash,
        compute_content_hash("name: test\nkind: persona\ndescription: Test!\n", body)
    );
}

#[test]
fn validation_errors_do_not_echo_persona_body() {
    let body = "PERSONA_BODY_MUST_NOT_APPEAR";
    let error = validate_persona_content("test-persona", &persona_content("other-persona", body))
        .expect_err("mismatched persona should fail");

    assert!(!error.to_string().contains(body));
}

#[test]
fn persona_composer_round_trips_special_character_descriptions() {
    let content = compose_persona_content(
        "design-voice",
        "Opinionated: \"product\" #1 😀",
        "Use direct, concrete language.",
    );

    let parsed = validate_persona_content("design-voice", &content)
        .expect("composed persona content should validate");

    assert_eq!(parsed.frontmatter.name, "design-voice");
    assert_eq!(parsed.frontmatter.kind.as_deref(), Some("persona"));
    assert_eq!(
        parsed.frontmatter.description,
        "Opinionated: \"product\" #1 😀"
    );
    // The canonical blank line after the closing `---` stays in the parsed body.
    assert_eq!(parsed.body, "\nUse direct, concrete language.\n");
}

#[test]
fn persona_composer_normalizes_description_newlines_and_preserves_body_content() {
    let body = "\nUse headings deliberately.\n\nKeep code examples short.\n";
    let content = compose_persona_content("design-voice", "  Calm\nfocused\r\nvoice  ", body);

    let parsed = validate_persona_content("design-voice", &content)
        .expect("composed persona content should validate");

    assert_eq!(parsed.frontmatter.description, "Calm focused voice");
    assert_eq!(
        parsed.body,
        "\nUse headings deliberately.\n\nKeep code examples short.\n"
    );
}
