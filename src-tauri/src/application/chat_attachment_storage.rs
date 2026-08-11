use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

use crate::domain::entities::{ChatAttachmentId, ChatConversationId};

const CHAT_ATTACHMENTS_DIR: &str = "chat_attachments";
const ATTACHMENT_STORAGE_DIR: &str = "attachments";
const CONTENT_FILE_STEM: &str = "content";
const WORKSPACE_ATTACHMENTS_DIR: &str = "attachments";

pub fn chat_attachment_storage_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(ATTACHMENT_STORAGE_DIR)
}

pub fn build_chat_attachment_file_path(
    base_path: &Path,
    conversation_id: &ChatConversationId,
    attachment_id: &ChatAttachmentId,
    file_name: &str,
) -> Result<PathBuf, String> {
    let content_file_name = attachment_content_file_name(file_name)?;

    Ok(base_path
        .join(CHAT_ATTACHMENTS_DIR)
        .join(hashed_component("conversation", &conversation_id.as_str()))
        .join(hashed_component("attachment", &attachment_id.as_str()))
        .join(content_file_name))
}

pub fn build_builder_workspace_attachment_path(
    workspace_root: &Path,
    attachment_id: &ChatAttachmentId,
    file_name: &str,
) -> Result<PathBuf, String> {
    let content_file_name = attachment_content_file_name(file_name)?;
    Ok(workspace_root
        .join(WORKSPACE_ATTACHMENTS_DIR)
        .join(hashed_component("attachment", &attachment_id.as_str()))
        .join(content_file_name))
}

fn attachment_content_file_name(file_name: &str) -> Result<String, String> {
    validate_attachment_file_name(file_name)?;

    let extension = Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|extension| is_safe_extension(extension));

    Ok(match extension {
        Some(extension) => format!("{CONTENT_FILE_STEM}.{extension}"),
        None => CONTENT_FILE_STEM.to_string(),
    })
}

fn validate_attachment_file_name(file_name: &str) -> Result<(), String> {
    if file_name.is_empty() || file_name.contains('/') || file_name.contains('\\') {
        return Err("Attachment file name must be a single file name".to_string());
    }

    let mut components = Path::new(file_name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err("Attachment file name must be a single file name".to_string()),
    }
}

fn is_safe_extension(extension: &str) -> bool {
    !extension.is_empty()
        && extension.len() <= 16
        && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn hashed_component(prefix: &str, value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let mut encoded = String::with_capacity(24);
    for byte in &digest[..12] {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    format!("{prefix}-{encoded}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_chat_attachment_file_path_uses_safe_components() {
        let conversation_id =
            ChatConversationId::from_string("../target-project-conversation".to_string());
        let attachment_id = ChatAttachmentId::new();

        let path = build_chat_attachment_file_path(
            Path::new("/app-data/attachments"),
            &conversation_id,
            &attachment_id,
            "Screenshot.PNG",
        )
        .expect("path should be built");

        let rendered = path.to_string_lossy();
        assert!(rendered.starts_with("/app-data/attachments/chat_attachments/conversation-"));
        assert!(rendered.contains("/attachment-"));
        assert!(rendered.ends_with("/content.png"));
        assert!(!rendered.contains("target-project-conversation"));
        assert!(!rendered.contains("Screenshot.PNG"));
    }

    #[test]
    fn build_chat_attachment_file_path_uses_content_leaf_without_safe_extension() {
        let conversation_id = ChatConversationId::new();
        let attachment_id = ChatAttachmentId::new();

        let path = build_chat_attachment_file_path(
            Path::new("/app-data/attachments"),
            &conversation_id,
            &attachment_id,
            "README",
        )
        .expect("path should be built");

        assert!(path.ends_with("content"));
    }

    #[test]
    fn build_chat_attachment_file_path_rejects_path_like_file_names() {
        let conversation_id = ChatConversationId::new();
        let attachment_id = ChatAttachmentId::new();

        for file_name in [
            "../secret.txt",
            "/tmp/secret.txt",
            "nested/file.txt",
            "nested\\file.txt",
            ".",
            "",
        ] {
            let result = build_chat_attachment_file_path(
                Path::new("/app-data/attachments"),
                &conversation_id,
                &attachment_id,
                file_name,
            );
            assert!(result.is_err(), "{file_name:?} should be rejected");
        }
    }
}
