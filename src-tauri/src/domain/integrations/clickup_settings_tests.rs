use super::{
    ClickUpIntegrationSettings, DEFAULT_CLICKUP_BRANCH_NAME_TEMPLATE,
    DEFAULT_CLICKUP_COMMIT_SUBJECT_TEMPLATE, DEFAULT_CLICKUP_PR_TITLE_TEMPLATE,
};

#[test]
fn defaults_keep_strict_git_naming_opt_in() {
    let settings = ClickUpIntegrationSettings::default();

    assert!(!settings.strict_git_naming_enabled);
    assert_eq!(
        settings.branch_name_template,
        DEFAULT_CLICKUP_BRANCH_NAME_TEMPLATE
    );
    assert_eq!(
        settings.commit_subject_template,
        DEFAULT_CLICKUP_COMMIT_SUBJECT_TEMPLATE
    );
    assert_eq!(
        settings.pr_title_template,
        DEFAULT_CLICKUP_PR_TITLE_TEMPLATE
    );
}
