use super::ticket_git_convention::{
    disambiguate_branch_name, TicketGitConventionContext, TicketGitConventionError,
    TicketGitConventionTemplateKind, TicketGitConventionTemplates, MAX_TICKET_BRANCH_BYTES,
};

fn context<'a>(
    task_id: &'a str,
    task_name: &'a str,
    username: Option<&'a str>,
    summary: Option<&'a str>,
) -> TicketGitConventionContext<'a> {
    TicketGitConventionContext {
        task_id,
        task_name,
        username,
        summary,
    }
}

#[test]
fn clickup_defaults_render_branch_commit_and_pr_values() {
    let templates = TicketGitConventionTemplates::clickup_defaults();

    let rendered = templates
        .render(&context(
            "CU-123",
            "Fix Login / Redirect",
            Some("Ada Lovelace"),
            None,
        ))
        .expect("default convention should render");

    assert_eq!(
        rendered.branch_name,
        "cu-123_fix-login-redirect_ada-lovelace"
    );
    assert_eq!(rendered.commit_subject, "CU-123 - Fix Login / Redirect");
    assert_eq!(rendered.pr_title, "CU-123 - Fix Login / Redirect");
}

#[test]
fn every_template_requires_task_id_and_rejects_unknown_placeholders() {
    let missing_task_id = TicketGitConventionTemplates::new(
        ":taskName:",
        ":taskId: - :taskName:",
        ":taskId: - :taskName:",
    )
    .expect_err("branch template without task id must fail");
    assert!(matches!(
        missing_task_id,
        TicketGitConventionError::MissingTaskId {
            kind: TicketGitConventionTemplateKind::Branch
        }
    ));

    let unknown = TicketGitConventionTemplates::new(
        ":taskId:",
        ":taskId: - :ticketTitle:",
        ":taskId: - :taskName:",
    )
    .expect_err("unknown placeholder must fail");
    assert!(matches!(
        unknown,
        TicketGitConventionError::UnknownPlaceholder {
            kind: TicketGitConventionTemplateKind::CommitSubject,
            ..
        }
    ));

    let typo = TicketGitConventionTemplates::new(
        ":taskId:",
        ":taskId: - :task-name:",
        ":taskId: - :taskName:",
    )
    .expect_err("placeholder-shaped typos must not become literal text");
    assert!(matches!(
        typo,
        TicketGitConventionError::UnknownPlaceholder {
            kind: TicketGitConventionTemplateKind::CommitSubject,
            ..
        }
    ));
}

#[test]
fn summary_is_dynamic_for_commit_subjects_but_forbidden_in_branches() {
    let branch_error = TicketGitConventionTemplates::new(
        ":taskId:_:summary:",
        ":taskId: - :taskName:",
        ":taskId: - :taskName:",
    )
    .expect_err("summary must not affect the stable branch");
    assert!(matches!(
        branch_error,
        TicketGitConventionError::PlaceholderNotAllowed {
            kind: TicketGitConventionTemplateKind::Branch,
            ..
        }
    ));

    let templates = TicketGitConventionTemplates::new(
        ":taskId:_:taskName:",
        ":taskId: - :summary:",
        ":taskId: - :taskName:",
    )
    .unwrap();
    let base = context("CU-123", "Fix login", None, None);

    assert!(templates
        .commit_subject_matches(&base, "CU-123 - Explain retry behavior")
        .unwrap());
    assert!(!templates
        .commit_subject_matches(&base, "CU-999 - Explain retry behavior")
        .unwrap());
    assert!(!templates
        .commit_subject_matches(&base, "CU-123 - ")
        .unwrap());
}

#[test]
fn username_is_required_only_when_the_selected_templates_use_it() {
    let without_username = TicketGitConventionTemplates::new(
        ":taskId:_:taskName:",
        ":taskId: - :taskName:",
        ":taskId: - :taskName:",
    )
    .unwrap();
    without_username
        .render(&context("CU-123", "Fix login", None, None))
        .expect("templates without username should render without current user");

    let defaults = TicketGitConventionTemplates::clickup_defaults();
    let error = defaults
        .render(&context("CU-123", "Fix login", None, None))
        .expect_err("default branch needs the authenticated ClickUp username");
    assert!(matches!(
        error,
        TicketGitConventionError::MissingPlaceholderValue { placeholder } if placeholder == "username"
    ));
}

#[test]
fn unsafe_branch_literals_fail_closed() {
    for branch_template in [
        "../:taskId:",
        "/:taskId:",
        ":taskId:.lock",
        "work/.:taskId:",
        ":taskId: @{ :taskName:",
        ":taskId:/:taskName:/",
    ] {
        let result = TicketGitConventionTemplates::new(
            branch_template,
            ":taskId: - :taskName:",
            ":taskId: - :taskName:",
        );
        assert!(
            result.is_err(),
            "expected unsafe branch reject: {branch_template}"
        );
    }
}

#[test]
fn long_unicode_branches_are_byte_bounded_and_deterministic() {
    let templates = TicketGitConventionTemplates::new(
        ":taskId:_:taskName:_:username:",
        ":taskId: - :taskName:",
        ":taskId: - :taskName:",
    )
    .unwrap();
    let title = "Überprüfung-安全-".repeat(80);
    let input = context("CU-123", &title, Some("Zoë"), None);

    let first = templates.render(&input).unwrap().branch_name;
    let second = templates.render(&input).unwrap().branch_name;

    assert_eq!(first, second);
    assert!(first.len() <= MAX_TICKET_BRANCH_BYTES);
    assert!(first.is_char_boundary(first.len()));
    let suffix = first.rsplit_once('-').unwrap().1;
    assert_eq!(suffix.len(), 8, "truncated branches carry a short hash");
    assert!(suffix
        .chars()
        .all(|character| character.is_ascii_hexdigit()));
}

#[test]
fn collision_disambiguation_is_explicit_and_stable() {
    let original = "cu-123_fix-login_ada";

    let first = disambiguate_branch_name(original, "clickup:workspace-1:task-123").unwrap();
    let second = disambiguate_branch_name(original, "clickup:workspace-1:task-123").unwrap();
    let other = disambiguate_branch_name(original, "clickup:workspace-1:task-456").unwrap();

    assert_eq!(first, second);
    assert_ne!(first, other);
    assert!(first.starts_with(original));
    assert!(first.len() <= MAX_TICKET_BRANCH_BYTES);
}

#[test]
fn subjects_and_titles_must_be_single_line() {
    let commit_error = TicketGitConventionTemplates::new(
        ":taskId:",
        ":taskId:\n:taskName:",
        ":taskId: - :taskName:",
    )
    .expect_err("commit subject template must be one line");
    assert!(matches!(
        commit_error,
        TicketGitConventionError::InvalidTemplate {
            kind: TicketGitConventionTemplateKind::CommitSubject,
            ..
        }
    ));

    let templates = TicketGitConventionTemplates::new(
        ":taskId:",
        ":taskId: - :summary:",
        ":taskId: - :taskName:",
    )
    .unwrap();
    assert!(!templates
        .commit_subject_matches(
            &context("CU-123", "Fix login", None, None),
            "CU-123 - bad\tmessage",
        )
        .unwrap());
}
