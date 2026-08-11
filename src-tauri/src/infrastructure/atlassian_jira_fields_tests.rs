use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use hyper::Method;
use serde_json::{json, Value};

use super::atlassian_client::{
    AtlassianAuthContext, AtlassianCredential, AtlassianJsonRequester, RequestAuth,
};
use super::atlassian_jira_fields::{
    acceptance_criteria_field_ids, acceptance_criteria_from_fields, fetch_jira_field_catalog,
    JiraFieldCatalogCache, JiraFieldDescriptor,
};

#[derive(Default)]
struct FakeRequester {
    responses: Mutex<VecDeque<Result<Value, String>>>,
    requests: Mutex<Vec<String>>,
}

impl FakeRequester {
    fn new(responses: Vec<Result<Value, String>>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests").clone()
    }
}

#[async_trait]
impl AtlassianJsonRequester for FakeRequester {
    async fn request_json(
        &self,
        method: Method,
        url: String,
        _auth: RequestAuth<'_>,
        _body: Option<Value>,
    ) -> Result<Value, String> {
        assert_eq!(method, Method::GET);
        self.requests.lock().expect("requests").push(url);
        self.responses
            .lock()
            .expect("responses")
            .pop_front()
            .unwrap_or_else(|| Err("unexpected Atlassian request".to_string()))
    }
}

fn auth_context() -> AtlassianAuthContext {
    AtlassianAuthContext {
        site_url: "https://example.atlassian.net".to_string(),
        credential: AtlassianCredential::ApiToken {
            email: "dev@example.com".to_string(),
            token: "token".to_string(),
        },
    }
}

#[test]
fn acceptance_criteria_field_matching_is_conservative_deterministic_and_bounded() {
    let fields = vec![
        JiraFieldDescriptor {
            id: "customfield_9".into(),
            name: "Acceptance Criteria (s)".into(),
            custom: true,
        },
        JiraFieldDescriptor {
            id: "customfield_4".into(),
            name: "Acceptance Criteria".into(),
            custom: true,
        },
        JiraFieldDescriptor {
            id: "customfield_2".into(),
            name: "acceptance criteria:".into(),
            custom: true,
        },
        JiraFieldDescriptor {
            id: "customfield_8".into(),
            name: "Acceptance Criterion".into(),
            custom: true,
        },
        JiraFieldDescriptor {
            id: "customfield_1".into(),
            name: "AC".into(),
            custom: true,
        },
        JiraFieldDescriptor {
            id: "customfield_3".into(),
            name: "Story Points".into(),
            custom: true,
        },
    ];

    assert_eq!(
        acceptance_criteria_field_ids(&fields),
        vec![
            "customfield_4".to_string(),
            "customfield_2".to_string(),
            "customfield_8".to_string(),
        ]
    );
}

#[test]
fn acceptance_criteria_values_support_adf_strings_and_option_arrays() {
    let ids = vec!["missing".to_string(), "ac".to_string()];
    let adf = json!({
        "ac": {
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "content": [{ "type": "text", "text": "Ship the parser" }]
            }]
        }
    });
    let string = json!({ "ac": "  Plain acceptance criteria  " });
    let object_array = json!({ "ac": [{ "value": "First" }, { "name": "Second" }] });
    let string_array = json!({ "ac": ["Alpha", "Beta"] });

    let rich = acceptance_criteria_from_fields(&adf, &ids).expect("ADF criteria");
    assert_eq!(rich.markdown, "Ship the parser");
    assert_eq!(rich.text, "Ship the parser");
    assert_eq!(
        acceptance_criteria_from_fields(&string, &ids)
            .expect("string criteria")
            .markdown,
        "Plain acceptance criteria"
    );
    assert_eq!(
        acceptance_criteria_from_fields(&object_array, &ids)
            .expect("object array criteria")
            .markdown,
        "- First\n- Second"
    );
    assert_eq!(
        acceptance_criteria_from_fields(&string_array, &ids)
            .expect("string array criteria")
            .markdown,
        "- Alpha\n- Beta"
    );
    assert!(acceptance_criteria_from_fields(&json!({ "ac": null }), &ids).is_none());
    assert!(acceptance_criteria_from_fields(&json!({ "ac": "   " }), &ids).is_none());
}

#[tokio::test]
async fn field_catalog_fetch_parses_valid_descriptors_and_uses_jira_endpoint() {
    let requester = FakeRequester::new(vec![Ok(json!([
        { "id": "customfield_10037", "name": "Acceptance Criteria", "custom": true },
        { "id": "", "name": "Blank id" },
        { "id": "customfield_2", "name": " " }
    ]))]);

    let fields = fetch_jira_field_catalog(&requester, &auth_context())
        .await
        .expect("field catalog");

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].id, "customfield_10037");
    assert_eq!(
        requester.requests(),
        vec!["https://example.atlassian.net/rest/api/3/field".to_string()]
    );
}

#[tokio::test]
async fn catalog_failures_degrade_to_empty_and_are_not_cached() {
    let requester = FakeRequester::new(vec![
        Err("temporary failure".to_string()),
        Err("temporary failure".to_string()),
    ]);
    let cache = JiraFieldCatalogCache::new();

    assert!(cache
        .acceptance_criteria_ids(&requester, &auth_context())
        .await
        .is_empty());
    assert!(cache
        .acceptance_criteria_ids(&requester, &auth_context())
        .await
        .is_empty());
    assert_eq!(requester.requests().len(), 2);
}
