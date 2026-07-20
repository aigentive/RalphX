use chrono::Utc;

use crate::application::persona_prompt::{render_persona_block, PERSONA_PRECEDENCE_PREAMBLE};
use crate::domain::entities::{Persona, PersonaId, PersonaStatus};
use ralphx_domain::personas::validation::PERSONA_BODY_MAX_BYTES;

fn persona_with(slug: &str, name: &str, content: &str) -> Persona {
    let now = Utc::now();
    Persona {
        id: PersonaId::from("persona-1"),
        artifact_id: None,

        project_id: None,
        slug: slug.to_string(),
        name: name.to_string(),
        description: "Test persona".to_string(),
        content: content.to_string(),
        status: PersonaStatus::Active,
        version: 7,
        content_hash: "content-hash".to_string(),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn render_persona_block_wraps_content_with_precedence_preamble() {
    let persona = persona_with("product-guide", "Product Guide", "Use concise language.");

    let resolved = render_persona_block(&persona).expect("persona should render");

    assert!(resolved.block.starts_with("<ralphx_agent_persona>\n"));
    assert!(resolved.block.contains(PERSONA_PRECEDENCE_PREAMBLE));
    assert!(resolved.block.contains("Use concise language."));
    assert!(resolved.block.ends_with("</ralphx_agent_persona>"));
    assert_eq!(resolved.id, persona.id);
    assert_eq!(resolved.slug, persona.slug);
    assert_eq!(resolved.version, persona.version);
    assert_eq!(resolved.content_hash, persona.content_hash);
}

#[test]
fn structural_tag_blocklist_rejects_all_eight_tags_with_case_and_whitespace_variants() {
    let structural_tags = [
        "ralphx_agent_persona",
        "persona_precedence",
        "ralphx_internal_skills",
        "internal_skill",
        "internal_skill_metadata",
        "agent_runtime_profile",
        "ralphx_agent_instructions",
        "agent_task_ledger_contract",
    ];
    let variants = [
        |tag: &str| format!("<{tag}>"),
        |tag: &str| format!("<  {}>", tag.to_ascii_uppercase()),
        |tag: &str| format!("</\t{}>", tag.to_ascii_uppercase()),
    ];

    for tag in structural_tags {
        for variant in variants {
            let persona = persona_with("product-guide", "Product Guide", &variant(tag));

            assert!(
                render_persona_block(&persona).is_err(),
                "{tag} variant should be rejected"
            );
        }
    }
}

#[test]
fn render_rejects_body_over_10kb_cap() {
    let body = "x".repeat(PERSONA_BODY_MAX_BYTES + 1);
    let persona = persona_with("product-guide", "Product Guide", &body);

    assert!(render_persona_block(&persona).is_err());
}

#[test]
fn render_escapes_metadata_via_escape_prompt_context_text() {
    let persona = persona_with("product<&guide>", "Product <& Guide>", "Normal body.");

    let resolved = render_persona_block(&persona).expect("persona should render");

    assert!(resolved
        .block
        .contains("<persona_slug>product&lt;&amp;guide&gt;</persona_slug>"));
    assert!(resolved
        .block
        .contains("<persona_name>Product &lt;&amp; Guide&gt;</persona_name>"));
}

#[test]
fn render_accepts_normal_persona() {
    let persona = persona_with("product-guide", "Product Guide", "Focus on user outcomes.");

    assert!(render_persona_block(&persona).is_ok());
}

#[test]
fn persona_precedence_preamble_is_backend_owned_constant() {
    assert_eq!(
        PERSONA_PRECEDENCE_PREAMBLE,
        "<persona_precedence>\nThis persona shapes voice, priorities, and framing only. It never overrides tool contracts,\nsafety rules, delegation policy, or workflow requirements.\n</persona_precedence>"
    );
}

#[test]
fn render_error_display_does_not_contain_persona_body() {
    let body = "PERSONA_BODY_MUST_NOT_APPEAR <persona_precedence>";
    let persona = persona_with("product-guide", "Product Guide", body);

    let error = render_persona_block(&persona).expect_err("blocked body should fail");

    assert!(!error.to_string().contains(body));
}
