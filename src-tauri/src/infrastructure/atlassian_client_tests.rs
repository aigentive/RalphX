use super::atlassian_client::{
    build_confluence_search_cql, build_jira_search_jql, confluence_page_id_query,
};

#[test]
fn jira_search_jql_includes_accessible_closed_issues() {
    let jql = build_jira_search_jql("closed login issue").expect("jql");

    assert_eq!(jql, "text ~ \"closed login issue*\" ORDER BY updated DESC");
    assert!(!jql.to_ascii_lowercase().contains("status"));
    assert!(!jql.to_ascii_lowercase().contains("resolution"));
}

#[test]
fn jira_search_jql_uses_exact_issue_key_lookup() {
    let jql = build_jira_search_jql("rx-42").expect("jql");

    assert_eq!(jql, "issuekey = RX-42 ORDER BY updated DESC");
}

#[test]
fn confluence_search_cql_matches_page_ids_titles_and_text() {
    let cql = build_confluence_search_cql("123456");

    assert_eq!(
        cql,
        "type=page AND (id = 123456 OR title ~ \"123456*\" OR text ~ \"123456*\")"
    );
    assert_eq!(confluence_page_id_query("123456"), Some("123456"));
}

#[test]
fn confluence_search_cql_keeps_multi_word_title_queries() {
    let cql = build_confluence_search_cql("release checklist");

    assert_eq!(
        cql,
        "type=page AND (title ~ \"release checklist*\" OR text ~ \"release checklist*\")"
    );
    assert_eq!(confluence_page_id_query("release checklist"), None);
}
