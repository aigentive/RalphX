use std::path::Path;

use super::persona_ingest_conversation_path;

#[test]
fn conversation_path_preserves_the_legacy_disk_layout() {
    let storage = Path::new("/app-data/persona_ingest");

    assert_eq!(
        persona_ingest_conversation_path(storage, "legacy-conversation-id"),
        storage.join("conversation-ef215d79d8f34cef49b9627d")
    );
}
