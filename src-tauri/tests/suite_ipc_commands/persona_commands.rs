use ralphx_lib::commands::persona_commands::{
    CreatePersonaDraftInput, PersonaIdInput, UpdatePersonaInput,
};

#[test]
fn persona_commands_use_struct_param_wrapping() {
    let create: CreatePersonaDraftInput = serde_json::from_str(
        r#"{"slug":"persona-one","content":"body","sourceSessionId":"session-1"}"#,
    )
    .expect("camelCase create input should deserialize inside the input wrapper");
    assert_eq!(create.source_session_id.as_deref(), Some("session-1"));

    let update: UpdatePersonaInput =
        serde_json::from_str(r#"{"id":"persona-1","content":"updated"}"#)
            .expect("camelCase update input should deserialize inside the input wrapper");
    assert_eq!(update.id, "persona-1");

    let id: PersonaIdInput = serde_json::from_str(r#"{"id":"persona-1"}"#)
        .expect("id input should deserialize inside the input wrapper");
    assert_eq!(id.id, "persona-1");

    let snake_case: Result<CreatePersonaDraftInput, _> = serde_json::from_str(
        r#"{"slug":"persona-one","content":"body","source_session_id":"session-1"}"#,
    );
    assert!(
        snake_case.is_ok(),
        "optional unknown snake_case fields are ignored"
    );
    assert!(snake_case.unwrap().source_session_id.is_none());
}
