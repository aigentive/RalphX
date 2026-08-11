use super::*;

#[test]
fn ordered_overlay_reports_each_delivered_block_without_persona_coupling() {
    let folder_only = render_ordered_prompt_overlay(None, Some("<folder_refs />"));
    assert_eq!(folder_only.block.as_deref(), Some("<folder_refs />"));
    assert_eq!(
        folder_only.delivery(true),
        PromptOverlayDelivery {
            persona: false,
            folder_refs: true,
        }
    );

    let both = render_ordered_prompt_overlay(Some("<persona />"), Some("<folder_refs />"));
    assert_eq!(
        both.block.as_deref(),
        Some("<persona />\n\n<folder_refs />")
    );
    assert_eq!(
        both.delivery(false),
        PromptOverlayDelivery {
            persona: false,
            folder_refs: false,
        }
    );
}
