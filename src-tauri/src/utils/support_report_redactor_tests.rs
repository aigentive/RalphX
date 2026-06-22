use std::path::PathBuf;

use super::{redact_support_report_text, SupportReportRedactionContext};

#[test]
fn redacts_support_report_paths_urls_emails_and_tokens() {
    let context = SupportReportRedactionContext {
        project_root: Some(PathBuf::from("/Users/alice/company/private-repo")),
        workspace_root: Some(PathBuf::from(
            "/Users/alice/ralphx-worktrees/private-workspace",
        )),
        home_dir: Some(PathBuf::from("/Users/alice")),
    };
    let input = "\
project=/Users/alice/company/private-repo
workspace=/Users/alice/ralphx-worktrees/private-workspace
contact=alice@example.com
remote=git@github.example.com:secret/proprietary.git
callback=https://github.example.com/secret/proprietary?token=abc
OPENAI_API_KEY=sk-AAAAAAAAAAAAAAAAAAAAAAAA
";

    let redacted = redact_support_report_text(input, &context);

    assert!(redacted.text.contains("[PROJECT_ROOT]"));
    assert!(redacted.text.contains("[AGENT_WORKSPACE]"));
    assert!(redacted.text.contains("[REDACTED_EMAIL]"));
    assert!(redacted.text.contains("[REDACTED_GIT_REMOTE]"));
    assert!(redacted.text.contains("[REDACTED_URL]"));
    assert!(redacted.text.contains("OPENAI_API_KEY=***REDACTED***"));
    assert!(!redacted.text.contains("alice@example.com"));
    assert!(!redacted.text.contains("proprietary"));
    assert!(!redacted.text.contains("sk-AAAAAAAA"));
    assert!(!redacted.summary.replacements.is_empty());
}

#[test]
fn redacts_unknown_user_home_prefix_even_without_context_home() {
    let context = SupportReportRedactionContext::default();
    let redacted = redact_support_report_text("path=/Users/bob/project/file.rs", &context);

    assert_eq!(redacted.text, "path=$HOME/project/file.rs");
    assert!(redacted
        .summary
        .replacements
        .iter()
        .any(|entry| entry.category == "home_path" && entry.count == 1));
}
