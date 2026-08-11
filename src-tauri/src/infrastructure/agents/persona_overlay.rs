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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPromptOverlay {
    pub block: Option<String>,
    pub persona_requested: bool,
    pub folder_refs_requested: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptOverlayDelivery {
    pub persona: bool,
    pub folder_refs: bool,
}

impl RenderedPromptOverlay {
    pub fn delivery(&self, overlay_delivered: bool) -> PromptOverlayDelivery {
        PromptOverlayDelivery {
            persona: self.persona_requested && overlay_delivered,
            folder_refs: self.folder_refs_requested && overlay_delivered,
        }
    }
}

pub fn render_ordered_prompt_overlay(
    persona_block: Option<&str>,
    folder_refs_block: Option<&str>,
) -> RenderedPromptOverlay {
    RenderedPromptOverlay {
        block: render_ordered_prompt_overlay_block(persona_block, folder_refs_block),
        persona_requested: persona_block.is_some(),
        folder_refs_requested: folder_refs_block.is_some(),
    }
}

#[cfg(test)]
#[path = "persona_overlay_tests.rs"]
mod tests;
