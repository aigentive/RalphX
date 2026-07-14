use std::fs;
use std::path::Path;
use std::sync::Mutex;

use async_trait::async_trait;

use super::app_paths::AppPaths;
use super::ticket_attachment::{
    build_ticket_attachment_content_location, fetch_ticket_attachment_content,
    BoundedTicketAttachmentBytes, TicketAttachmentContentLocation, TicketAttachmentContentPointer,
    TicketAttachmentContentStore, TicketAttachmentDescriptor, TicketAttachmentError,
    TicketAttachmentListResult, TicketAttachmentProvider, TicketAttachmentProviderItem,
    TicketAttachmentProviderReader, TicketAttachmentSourceHandle,
    MAX_TICKET_ATTACHMENT_CONTENT_BYTES,
};
use super::ticket_attachment_runtime_store::TicketAttachmentRuntimeStore;

#[tokio::test]
async fn runtime_store_persists_bounded_content_under_app_owned_hashed_root() {
    let temp = tempfile::tempdir().expect("tempdir");
    let app_paths = AppPaths::new(temp.path().join("app-data"), None);
    let store = TicketAttachmentRuntimeStore::from_app_paths(&app_paths);
    let source = TicketAttachmentSourceHandle::new(
        TicketAttachmentProvider::ClickUp,
        "target-project-task",
        "att-123",
    )
    .expect("source");
    let bytes = BoundedTicketAttachmentBytes::new(b"ticket attachment bytes".to_vec())
        .expect("bounded bytes");

    let location = store
        .persist_content(&source, "Screenshot.PNG", &bytes)
        .await
        .expect("content should persist");

    let rendered = location.path().to_string_lossy();
    assert!(location.path().starts_with(store.attachment_root()));
    assert!(rendered.contains("/ticket_attachments/provider-clickup/"));
    assert!(rendered.ends_with("/content.png"));
    assert!(!rendered.contains("target-project-task"));
    assert!(!rendered.contains("att-123"));
    assert!(!rendered.contains("Screenshot.PNG"));
    assert_eq!(
        fs::read(location.path()).expect("content should be readable"),
        bytes.as_slice()
    );
}

#[tokio::test]
async fn runtime_store_rejects_path_like_names_without_writing_content() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = TicketAttachmentRuntimeStore::new(temp.path().join("app-data").join("attachments"));
    let source =
        TicketAttachmentSourceHandle::new(TicketAttachmentProvider::Jira, "JRA-1", "att-1")
            .expect("source");
    let bytes = BoundedTicketAttachmentBytes::new(vec![1, 2, 3]).expect("bounded bytes");

    let result = store
        .persist_content(&source, "../secret.txt", &bytes)
        .await;

    assert!(matches!(
        result,
        Err(TicketAttachmentError::UnsafeField { field: "file_name" })
    ));
    assert!(
        !store.attachment_root().exists(),
        "unsafe filename should fail before creating runtime storage"
    );
}

#[tokio::test]
async fn runtime_store_rejects_symlink_escape_and_leaves_no_final_content() {
    let temp = tempfile::tempdir().expect("tempdir");
    let app_root = temp.path().join("app-data").join("attachments");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&app_root).expect("app root");
    fs::create_dir_all(&outside).expect("outside");

    let store = TicketAttachmentRuntimeStore::new(&app_root);
    let source =
        TicketAttachmentSourceHandle::new(TicketAttachmentProvider::Linear, "LIN-1", "att-1")
            .expect("source");
    let location = build_ticket_attachment_content_location(&app_root, &source, "artifact.txt")
        .expect("location");
    let parent = location.path().parent().expect("content parent");
    let ticket_parent = parent.parent().expect("ticket parent");
    let provider_parent = ticket_parent.parent().expect("provider parent");
    fs::create_dir_all(provider_parent).expect("provider parent");

    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, ticket_parent).expect("symlink");

    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&outside, ticket_parent).expect("symlink");

    let bytes = BoundedTicketAttachmentBytes::new(vec![9, 9, 9]).expect("bounded bytes");
    let result = store.persist_content(&source, "artifact.txt", &bytes).await;

    assert!(matches!(
        result,
        Err(TicketAttachmentError::PathEscapedRoot)
            | Err(TicketAttachmentError::StorageRootUnavailable)
    ));
    assert!(
        !location.path().exists(),
        "escaped final path should not be reported as cached"
    );
    assert!(
        fs::read_dir(&outside)
            .expect("outside dir should remain readable")
            .next()
            .is_none(),
        "runtime store must not create directories through an escaping symlink"
    );
}

#[tokio::test]
async fn fetch_content_resolves_current_pointer_fetches_with_cap_and_persists_once() {
    let item = provider_item(true, Some(3));
    let pointer = item
        .source
        .content_pointer()
        .expect("pointer should be valid");
    let reader = RecordingReader::new(vec![item], Ok(vec![4, 5, 6]));
    let store = RecordingStore::default();

    let result = fetch_ticket_attachment_content(
        &reader,
        &store,
        TicketAttachmentProvider::Jira,
        "JRA-1",
        &pointer,
    )
    .await
    .expect("eligible content should fetch");

    assert_eq!(reader.fetch_count(), 1);
    assert_eq!(
        reader.last_max_bytes(),
        Some(MAX_TICKET_ATTACHMENT_CONTENT_BYTES)
    );
    assert_eq!(store.persist_count(), 1);
    assert_eq!(result.descriptor.content_pointer, pointer);
    assert!(result.location.is_some());
}

#[tokio::test]
async fn fetch_content_rejects_stale_or_unsupported_pointer_before_provider_fetch() {
    let current = provider_item(false, Some(3));
    let stale_pointer = TicketAttachmentContentPointer::new(
        TicketAttachmentProvider::Jira,
        "JRA-1",
        "other-attachment",
    )
    .expect("stale pointer");
    let reader = RecordingReader::new(vec![current.clone()], Ok(vec![4, 5, 6]));
    let store = RecordingStore::default();

    let stale = fetch_ticket_attachment_content(
        &reader,
        &store,
        TicketAttachmentProvider::Jira,
        "JRA-1",
        &stale_pointer,
    )
    .await;

    assert!(matches!(stale, Err(TicketAttachmentError::PointerNotFound)));
    assert_eq!(reader.fetch_count(), 0);
    assert_eq!(store.persist_count(), 0);

    let unsupported_pointer = current.source.content_pointer().expect("pointer");
    let unsupported = fetch_ticket_attachment_content(
        &reader,
        &store,
        TicketAttachmentProvider::Jira,
        "JRA-1",
        &unsupported_pointer,
    )
    .await;

    assert!(matches!(
        unsupported,
        Err(TicketAttachmentError::UnsupportedContentFetch)
    ));
    assert_eq!(reader.fetch_count(), 0);
    assert_eq!(store.persist_count(), 0);
}

#[tokio::test]
async fn fetch_content_rejects_cross_scope_pointer_before_provider_fetch() {
    let current = provider_item(true, Some(3));
    let cross_ticket_pointer =
        TicketAttachmentContentPointer::new(TicketAttachmentProvider::Jira, "JRA-2", "att-1")
            .expect("cross-ticket pointer");
    let cross_provider_pointer =
        TicketAttachmentContentPointer::new(TicketAttachmentProvider::Linear, "JRA-1", "att-1")
            .expect("cross-provider pointer");
    let reader = RecordingReader::new(vec![current], Ok(vec![4, 5, 6]));
    let store = RecordingStore::default();

    for pointer in [&cross_ticket_pointer, &cross_provider_pointer] {
        let result = fetch_ticket_attachment_content(
            &reader,
            &store,
            TicketAttachmentProvider::Jira,
            "JRA-1",
            pointer,
        )
        .await;

        assert!(matches!(
            result,
            Err(TicketAttachmentError::PointerNotFound)
        ));
        assert_eq!(reader.fetch_count(), 0);
        assert_eq!(store.persist_count(), 0);
    }
}

#[tokio::test]
async fn fetch_content_rechecks_declared_size_before_provider_fetch() {
    let item = provider_item(true, Some(MAX_TICKET_ATTACHMENT_CONTENT_BYTES as u64 + 1));
    let pointer = item.source.content_pointer().expect("pointer");
    let reader = RecordingReader::new(vec![item], Ok(vec![4, 5, 6]));
    let store = RecordingStore::default();

    let result = fetch_ticket_attachment_content(
        &reader,
        &store,
        TicketAttachmentProvider::Jira,
        "JRA-1",
        &pointer,
    )
    .await;

    assert!(matches!(
        result,
        Err(TicketAttachmentError::ContentTooLarge { .. })
    ));
    assert_eq!(reader.fetch_count(), 0);
    assert_eq!(store.persist_count(), 0);
}

#[tokio::test]
async fn fetch_content_does_not_persist_provider_failures() {
    let item = provider_item(true, Some(3));
    let pointer = item.source.content_pointer().expect("pointer");
    let reader = RecordingReader::new(
        vec![item],
        Err(TicketAttachmentError::ProviderRequestFailed),
    );
    let store = RecordingStore::default();

    let result = fetch_ticket_attachment_content(
        &reader,
        &store,
        TicketAttachmentProvider::Jira,
        "JRA-1",
        &pointer,
    )
    .await;

    assert!(matches!(
        result,
        Err(TicketAttachmentError::ProviderRequestFailed)
    ));
    assert_eq!(reader.fetch_count(), 1);
    assert_eq!(store.persist_count(), 0);
}

#[tokio::test]
async fn fetch_content_does_not_persist_over_limit_downloads() {
    let item = provider_item(true, Some(3));
    let pointer = item.source.content_pointer().expect("pointer");
    let reader = RecordingReader::new(
        vec![item],
        Ok(vec![4; MAX_TICKET_ATTACHMENT_CONTENT_BYTES + 1]),
    );
    let store = RecordingStore::default();

    let result = fetch_ticket_attachment_content(
        &reader,
        &store,
        TicketAttachmentProvider::Jira,
        "JRA-1",
        &pointer,
    )
    .await;

    assert!(matches!(
        result,
        Err(TicketAttachmentError::ContentTooLarge { .. })
    ));
    assert_eq!(reader.fetch_count(), 1);
    assert_eq!(store.persist_count(), 0);
}

#[tokio::test]
async fn runtime_store_cleans_temporary_file_when_atomic_rename_fails() {
    let temp = tempfile::tempdir().expect("tempdir");
    let app_root = temp.path().join("app-data").join("attachments");
    let store = TicketAttachmentRuntimeStore::new(&app_root);
    let source =
        TicketAttachmentSourceHandle::new(TicketAttachmentProvider::Linear, "LIN-1", "att-1")
            .expect("source");
    let location = build_ticket_attachment_content_location(&app_root, &source, "artifact.txt")
        .expect("location");
    let parent = location.path().parent().expect("content parent");
    fs::create_dir_all(parent).expect("content parent should be created");
    fs::create_dir(location.path()).expect("final path directory should force rename failure");
    let bytes = BoundedTicketAttachmentBytes::new(vec![9, 9, 9]).expect("bounded bytes");

    let result = store.persist_content(&source, "artifact.txt", &bytes).await;

    assert!(matches!(
        result,
        Err(TicketAttachmentError::StorageWriteFailed)
    ));
    let leaked_temp = fs::read_dir(parent)
        .expect("content parent should remain readable")
        .filter_map(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy().starts_with(".content-"));
    assert!(!leaked_temp, "failed atomic writes must remove temp files");
    assert!(
        location.path().is_dir(),
        "failed rename must not replace the existing final path"
    );
}

fn provider_item(
    content_fetch_supported: bool,
    declared_size_bytes: Option<u64>,
) -> TicketAttachmentProviderItem {
    let source =
        TicketAttachmentSourceHandle::new(TicketAttachmentProvider::Jira, "JRA-1", "att-1")
            .expect("source");
    let mut descriptor = TicketAttachmentDescriptor::new(
        TicketAttachmentProvider::Jira,
        "JRA-1",
        "att-1",
        "evidence.txt",
        Some("text/plain"),
        None,
        None,
    )
    .expect("descriptor");
    descriptor.declared_size_bytes = declared_size_bytes;

    TicketAttachmentProviderItem::new(descriptor, source, content_fetch_supported)
}

struct RecordingReader {
    items: Vec<TicketAttachmentProviderItem>,
    fetch_result: Result<Vec<u8>, TicketAttachmentError>,
    fetch_count: Mutex<usize>,
    last_max_bytes: Mutex<Option<usize>>,
}

impl RecordingReader {
    fn new(
        items: Vec<TicketAttachmentProviderItem>,
        fetch_result: Result<Vec<u8>, TicketAttachmentError>,
    ) -> Self {
        Self {
            items,
            fetch_result,
            fetch_count: Mutex::new(0),
            last_max_bytes: Mutex::new(None),
        }
    }

    fn fetch_count(&self) -> usize {
        *self.fetch_count.lock().expect("fetch count")
    }

    fn last_max_bytes(&self) -> Option<usize> {
        *self.last_max_bytes.lock().expect("max bytes")
    }
}

#[async_trait]
impl TicketAttachmentProviderReader for RecordingReader {
    async fn list_attachments(
        &self,
        _provider: TicketAttachmentProvider,
        _ticket_id: &str,
    ) -> Result<TicketAttachmentListResult, TicketAttachmentError> {
        TicketAttachmentListResult::from_items(self.items.clone())
    }

    async fn fetch_attachment(
        &self,
        _source: &TicketAttachmentSourceHandle,
        max_bytes: usize,
    ) -> Result<BoundedTicketAttachmentBytes, TicketAttachmentError> {
        *self.fetch_count.lock().expect("fetch count") += 1;
        *self.last_max_bytes.lock().expect("max bytes") = Some(max_bytes);
        self.fetch_result
            .clone()
            .and_then(BoundedTicketAttachmentBytes::new)
    }
}

#[derive(Default)]
struct RecordingStore {
    persist_count: Mutex<usize>,
}

impl RecordingStore {
    fn persist_count(&self) -> usize {
        *self.persist_count.lock().expect("persist count")
    }
}

#[async_trait]
impl TicketAttachmentContentStore for RecordingStore {
    async fn content_location(
        &self,
        source: &TicketAttachmentSourceHandle,
        file_name: &str,
    ) -> Result<TicketAttachmentContentLocation, TicketAttachmentError> {
        build_ticket_attachment_content_location(
            Path::new("/app-data/attachments"),
            source,
            file_name,
        )
    }

    async fn persist_content(
        &self,
        source: &TicketAttachmentSourceHandle,
        file_name: &str,
        _bytes: &BoundedTicketAttachmentBytes,
    ) -> Result<TicketAttachmentContentLocation, TicketAttachmentError> {
        *self.persist_count.lock().expect("persist count") += 1;
        self.content_location(source, file_name).await
    }
}
