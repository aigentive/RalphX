/// Appends the already-rendered persona block to a harness system prompt.
pub fn apply_persona_overlay(system_prompt: String, persona_block: Option<&str>) -> String {
    match persona_block {
        Some(persona_block) => format!("{system_prompt}\n\n{persona_block}"),
        None => system_prompt,
    }
}
