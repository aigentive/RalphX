use super::agent_workspace_pr_metadata::AgentWorkspacePrMetadataDecision;

#[test]
fn patch_requires_one_non_empty_field_and_trims_title() {
    assert!(AgentWorkspacePrMetadataDecision::patch(None, None).is_none());
    assert!(AgentWorkspacePrMetadataDecision::patch(Some(" ".into()), Some("\n".into())).is_none());
    assert_eq!(
        AgentWorkspacePrMetadataDecision::patch(Some(" title ".into()), None),
        Some(AgentWorkspacePrMetadataDecision::Patch {
            title: Some("title".into()),
            body_markdown: None
        })
    );
}

#[test]
fn preserve_and_partial_patches_are_valid() {
    assert!(AgentWorkspacePrMetadataDecision::preserve().is_valid());
    assert!(
        AgentWorkspacePrMetadataDecision::patch(None, Some("body".into()))
            .unwrap()
            .is_valid()
    );
    assert!(!AgentWorkspacePrMetadataDecision::Patch {
        title: Some("  ".into()),
        body_markdown: Some("\n".into()),
    }
    .is_valid());
}
