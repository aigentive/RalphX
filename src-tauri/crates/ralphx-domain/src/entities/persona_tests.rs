use super::*;

#[test]
fn persona_directive_default_is_inherit() {
    assert_eq!(PersonaDirective::default(), PersonaDirective::Inherit);
}
