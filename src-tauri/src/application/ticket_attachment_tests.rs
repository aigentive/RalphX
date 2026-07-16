use std::fs;
use std::path::Path;

use super::ticket_attachment::{
    build_ticket_attachment_content_location, ensure_ticket_attachment_content_size,
    validate_ticket_attachment_content_parent, BoundedTicketAttachmentBytes,
    TicketAttachmentContentPointer, TicketAttachmentDescriptor, TicketAttachmentError,
    TicketAttachmentListResult, TicketAttachmentProvider, TicketAttachmentSourceHandle,
    MAX_TICKET_ATTACHMENT_CONTENT_BYTES, MAX_TICKET_ATTACHMENT_FILE_NAME_BYTES,
    MAX_TICKET_ATTACHMENT_LIST_ITEMS,
};

#[test]
fn descriptor_serialization_exposes_only_safe_metadata_and_opaque_pointer() {
    let descriptor = TicketAttachmentDescriptor::new(
        TicketAttachmentProvider::Jira,
        "JRA-123",
        "att-456",
        "crash.log",
        Some("text/plain"),
        Some(512),
        Some("2026-07-13T07:00:00Z".to_string()),
    )
    .expect("descriptor should be valid");

    let serialized = serde_json::to_string(&descriptor).expect("descriptor should serialize");

    assert!(serialized.contains("\"provider\":\"jira\""));
    assert!(serialized.contains("\"id\":\"ta_"));
    assert!(serialized.contains("\"fileName\":\"crash.log\""));
    assert!(serialized.contains("\"contentPointer\":{\"id\":\"ta_"));
    assert!(!serialized.contains("att-456"));
    assert!(!serialized.contains("http://"));
    assert!(!serialized.contains("https://"));
    assert!(!serialized.contains("Bearer"));
    assert!(!serialized.contains("/tmp"));
    assert!(!serialized.contains("content_url"));
}

#[test]
fn content_pointer_is_deterministic_and_opaque() {
    let first =
        TicketAttachmentContentPointer::new(TicketAttachmentProvider::Linear, "LIN-1", "att-1")
            .expect("pointer should be valid");
    let second =
        TicketAttachmentContentPointer::new(TicketAttachmentProvider::Linear, "LIN-1", "att-1")
            .expect("pointer should be valid");
    let other =
        TicketAttachmentContentPointer::new(TicketAttachmentProvider::ClickUp, "LIN-1", "att-1")
            .expect("pointer should be valid");

    assert_eq!(first, second);
    assert_ne!(first, other);
    assert!(first.id().starts_with("ta_"));
    assert!(!first.id().contains("LIN-1"));
    assert!(!first.id().contains("att-1"));
}

#[test]
fn content_pointer_from_id_accepts_only_opaque_pointer_ids() {
    let pointer = TicketAttachmentContentPointer::from_id("ta_123abc").unwrap();
    assert_eq!(pointer.id(), "ta_123abc");

    let url = TicketAttachmentContentPointer::from_id("https://example.test/download");
    assert!(matches!(
        url,
        Err(TicketAttachmentError::UnsafeField {
            field: "content_pointer"
        })
    ));

    let raw_id = TicketAttachmentContentPointer::from_id("jira-attachment-1");
    assert!(matches!(
        raw_id,
        Err(TicketAttachmentError::UnsafeField {
            field: "content_pointer"
        })
    ));
}

#[test]
fn identifiers_reject_urls_paths_and_oversized_values() {
    for ticket_id in [
        "https://example.test/secret",
        "Bearer token",
        "../ticket",
        "a/b",
    ] {
        let result =
            TicketAttachmentSourceHandle::new(TicketAttachmentProvider::Jira, ticket_id, "att-1");
        assert!(
            matches!(result, Err(TicketAttachmentError::UnsafeField { .. })),
            "{ticket_id:?} should be rejected"
        );
    }

    let oversized = "x".repeat(MAX_TICKET_ATTACHMENT_FILE_NAME_BYTES + 1);
    let result = TicketAttachmentDescriptor::new(
        TicketAttachmentProvider::Jira,
        "JRA-1",
        "att-1",
        &oversized,
        None,
        None,
        None,
    );

    assert!(matches!(
        result,
        Err(TicketAttachmentError::FieldTooLarge {
            field: "file_name",
            ..
        })
    ));
}

#[test]
fn content_size_limits_accept_exact_cap_and_reject_over_cap() {
    ensure_ticket_attachment_content_size(MAX_TICKET_ATTACHMENT_CONTENT_BYTES as u64)
        .expect("exact cap should be valid");

    let over =
        ensure_ticket_attachment_content_size(MAX_TICKET_ATTACHMENT_CONTENT_BYTES as u64 + 1);
    assert!(matches!(
        over,
        Err(TicketAttachmentError::ContentTooLarge { .. })
    ));

    let bounded =
        BoundedTicketAttachmentBytes::new(vec![7; 16]).expect("small byte buffer should be valid");
    assert_eq!(bounded.as_slice(), &[7; 16]);

    let oversized =
        BoundedTicketAttachmentBytes::new(vec![7; MAX_TICKET_ATTACHMENT_CONTENT_BYTES + 1]);
    assert!(matches!(
        oversized,
        Err(TicketAttachmentError::ContentTooLarge { .. })
    ));
}

#[test]
fn list_result_enforces_attachment_count_cap() {
    let mut attachments = Vec::with_capacity(MAX_TICKET_ATTACHMENT_LIST_ITEMS + 1);
    for index in 0..=MAX_TICKET_ATTACHMENT_LIST_ITEMS {
        attachments.push(
            TicketAttachmentDescriptor::new(
                TicketAttachmentProvider::ClickUp,
                "task-1",
                &format!("att-{index}"),
                "artifact.txt",
                Some("text/plain"),
                None,
                None,
            )
            .expect("descriptor should be valid"),
        );
    }

    let result = TicketAttachmentListResult::new(attachments);

    assert!(matches!(
        result,
        Err(TicketAttachmentError::TooManyAttachments { .. })
    ));
}

#[test]
fn storage_location_uses_hash_components_and_app_owned_root() {
    let source = TicketAttachmentSourceHandle::new(
        TicketAttachmentProvider::ClickUp,
        "../target-project-task",
        "att-123",
    );
    assert!(matches!(
        source,
        Err(TicketAttachmentError::UnsafeField { .. })
    ));

    let source = TicketAttachmentSourceHandle::new(
        TicketAttachmentProvider::ClickUp,
        "target-project-task",
        "att-123",
    )
    .expect("source should be valid");
    let location = build_ticket_attachment_content_location(
        Path::new("/app-data/attachments"),
        &source,
        "Screenshot.PNG",
    )
    .expect("location should be built");

    let rendered = location.path().to_string_lossy();
    assert!(rendered.starts_with("/app-data/attachments/ticket_attachments/provider-clickup/"));
    assert!(rendered.contains("/ticket-"));
    assert!(rendered.contains("/attachment-"));
    assert!(rendered.ends_with("/content.png"));
    assert!(!rendered.contains("target-project-task"));
    assert!(!rendered.contains("att-123"));
    assert!(!rendered.contains("Screenshot.PNG"));
}

#[test]
fn storage_location_rejects_path_like_file_names() {
    let source =
        TicketAttachmentSourceHandle::new(TicketAttachmentProvider::Jira, "JRA-1", "att-1")
            .expect("source should be valid");

    for file_name in [
        "../secret.txt",
        "/tmp/secret.txt",
        "nested/file.txt",
        "nested\\file.txt",
        ".",
        "",
    ] {
        let result =
            build_ticket_attachment_content_location(Path::new("/app-data"), &source, file_name);
        assert!(
            matches!(
                result,
                Err(TicketAttachmentError::UnsafeField { .. }
                    | TicketAttachmentError::EmptyField { .. })
            ),
            "{file_name:?} should be rejected"
        );
    }
}

#[test]
fn storage_parent_validation_rejects_symlink_escape() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let app_root = temp.path().join("app-owned");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&app_root).expect("app root should be created");
    fs::create_dir_all(&outside).expect("outside dir should be created");

    let source =
        TicketAttachmentSourceHandle::new(TicketAttachmentProvider::Linear, "LIN-1", "att-1")
            .expect("source should be valid");
    let location = build_ticket_attachment_content_location(&app_root, &source, "artifact.txt")
        .expect("location should be built");
    let parent = location
        .path()
        .parent()
        .expect("location should have parent");
    let provider_parent = parent
        .parent()
        .and_then(Path::parent)
        .expect("provider parent should exist");
    fs::create_dir_all(provider_parent).expect("provider parent should be created");
    fs::create_dir_all(outside.join(parent.file_name().expect("attachment dir name")))
        .expect("outside attachment dir should be created");

    #[cfg(unix)]
    std::os::unix::fs::symlink(
        &outside,
        parent.parent().expect("ticket parent should exist"),
    )
    .expect("symlink should be created");

    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(
        &outside,
        parent.parent().expect("ticket parent should exist"),
    )
    .expect("symlink should be created");

    let result = validate_ticket_attachment_content_parent(&app_root, &location);

    assert!(matches!(
        result,
        Err(TicketAttachmentError::PathEscapedRoot)
    ));
}

#[test]
fn error_messages_do_not_echo_sensitive_values() {
    let error = TicketAttachmentSourceHandle::new(
        TicketAttachmentProvider::Jira,
        "https://example.test/download?token=secret",
        "att-1",
    )
    .expect_err("unsafe URL-like ticket id should fail");

    let rendered = error.to_string();

    assert!(!rendered.contains("https://example.test"));
    assert!(!rendered.contains("token=secret"));
    assert!(!rendered.contains("att-1"));
}
