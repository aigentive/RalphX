use super::*;
use std::str::FromStr;

#[test]
fn persona_directive_default_is_inherit() {
    assert_eq!(PersonaDirective::default(), PersonaDirective::Inherit);
}

#[test]
fn persona_id_and_status_text_contracts_round_trip_and_reject_unknown_status() {
    let id = PersonaId::from("persona-text-contract");

    assert_eq!(id.as_str(), "persona-text-contract");
    assert_eq!(id.to_string(), "persona-text-contract");
    assert_eq!(
        PersonaId::from_string("from-string"),
        PersonaId::from("from-string")
    );
    assert_eq!(PersonaStatus::Draft.to_string(), "draft");
    assert_eq!(PersonaStatus::Active.to_string(), "active");
    assert_eq!(PersonaStatus::Archived.to_string(), "archived");
    assert_eq!(
        PersonaStatus::from_str("active").unwrap(),
        PersonaStatus::Active
    );
    assert!(PersonaStatus::from_str("retired")
        .expect_err("unknown persisted statuses must fail closed")
        .to_string()
        .contains("retired"));
}

#[test]
fn persona_bindability_tracks_only_active_status() {
    let persona = |status| Persona {
        id: PersonaId::from("bindability"),
        artifact_id: None,

        project_id: None,
        slug: "bindability".to_string(),
        name: "Bindability".to_string(),
        description: String::new(),
        content: String::new(),
        status,
        version: 1,
        content_hash: String::new(),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    assert!(persona(PersonaStatus::Active).is_bindable());
    assert!(!persona(PersonaStatus::Draft).is_bindable());
    assert!(!persona(PersonaStatus::Archived).is_bindable());
}
