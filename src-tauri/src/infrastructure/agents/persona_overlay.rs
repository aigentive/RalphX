/// Applies already-rendered prompt overlays in caller-defined order.
pub fn apply_prompt_overlays<'a>(
    mut system_prompt: String,
    overlays: impl IntoIterator<Item = Option<&'a str>>,
) -> String {
    for overlay in overlays.into_iter().flatten() {
        system_prompt.push_str("\n\n");
        system_prompt.push_str(overlay);
    }
    system_prompt
}

/// Appends the already-rendered persona block to a harness system prompt.
pub fn apply_persona_overlay(system_prompt: String, persona_block: Option<&str>) -> String {
    apply_prompt_overlays(system_prompt, [persona_block])
}

pub fn render_ordered_prompt_overlay_block(
    persona_block: Option<&str>,
    folder_refs_block: Option<&str>,
) -> Option<String> {
    let rendered = apply_prompt_overlays(String::new(), [persona_block, folder_refs_block]);
    (!rendered.is_empty()).then(|| rendered.trim_start().to_string())
}
