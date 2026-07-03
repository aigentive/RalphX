use ralphx_lib::application::app_state::AppState;
use ralphx_lib::commands::api_key_commands::{
    create_api_key, get_api_key_audit_log, list_api_keys, revoke_api_key, rotate_api_key,
    update_api_key_permissions, update_api_key_projects, CreateApiKeyInput, GetAuditLogInput,
    RevokeApiKeyInput, RotateApiKeyInput, UpdateApiKeyPermissionsInput,
    UpdateApiKeyProjectsInput,
};
use ralphx_lib::domain::entities::ApiKeyId;
use ralphx_lib::domain::services::api_key_service::{ApiKeyService, KeySource};
use tauri::Manager;

fn setup_test_state() -> AppState {
    AppState::new_test()
}

fn api_key_command_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

// ── create_key ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_create_api_key_returns_raw_key() {
    let state = setup_test_state();
    let repo = state.api_key_repo.as_ref();

    let created = ApiKeyService::create_key(repo, "test-key", None, &[], KeySource::SettingsUi)
        .await
        .unwrap();

    assert!(!created.raw_key.is_empty(), "raw_key must be returned");
    assert_eq!(created.key.name, "test-key");
}

#[tokio::test]
async fn test_create_api_key_default_permissions_settings_ui() {
    let state = setup_test_state();
    let repo = state.api_key_repo.as_ref();

    let created = ApiKeyService::create_key(repo, "admin-key", None, &[], KeySource::SettingsUi)
        .await
        .unwrap();

    // SettingsUi default = 7 (read + write + admin)
    assert_eq!(created.key.permissions, 7);
}

#[tokio::test]
async fn test_create_api_key_persists_to_repo() {
    let state = setup_test_state();
    let repo = state.api_key_repo.as_ref();

    let created = ApiKeyService::create_key(repo, "persist-key", None, &[], KeySource::SettingsUi)
        .await
        .unwrap();

    let keys = repo.list().await.unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].id, created.key.id);
}

#[tokio::test]
async fn create_api_key_command_returns_raw_key_and_project_associations() {
    let app = api_key_command_app();

    let created = create_api_key(
        app.state::<AppState>(),
        CreateApiKeyInput {
            name: "command-key".to_string(),
            permissions: Some(5),
            project_ids: Some(vec!["proj-a".to_string(), "proj-b".to_string()]),
        },
    )
    .await
    .expect("API key creates");

    assert_eq!(created.name, "command-key");
    assert_eq!(created.permissions, 5);
    assert!(!created.raw_key.is_empty());
    assert!(!created.key_prefix.is_empty());

    let keys = list_api_keys(app.state::<AppState>())
        .await
        .expect("API keys list");
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].id, created.id);
    assert_eq!(keys[0].name, "command-key");
    assert_eq!(keys[0].permissions, 5);
    assert_eq!(keys[0].project_ids, vec!["proj-a", "proj-b"]);
}

#[tokio::test]
async fn create_api_key_command_rejects_invalid_permissions() {
    let app = api_key_command_app();

    let error = create_api_key(
        app.state::<AppState>(),
        CreateApiKeyInput {
            name: "bad-permissions".to_string(),
            permissions: Some(99),
            project_ids: None,
        },
    )
    .await
    .expect_err("invalid permissions should error");

    assert!(error.contains("permissions must be between"));
}

// ── list_keys ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_api_keys_returns_all_active() {
    let state = setup_test_state();
    let repo = state.api_key_repo.as_ref();

    ApiKeyService::create_key(repo, "key-a", None, &[], KeySource::SettingsUi)
        .await
        .unwrap();
    ApiKeyService::create_key(repo, "key-b", None, &[], KeySource::SettingsUi)
        .await
        .unwrap();

    let keys = repo.list().await.unwrap();
    assert_eq!(keys.len(), 2);
}

#[tokio::test]
async fn list_api_keys_command_omits_revoked_keys_and_maps_optional_fields() {
    let app = api_key_command_app();

    let active = create_api_key(
        app.state::<AppState>(),
        CreateApiKeyInput {
            name: "active-key".to_string(),
            permissions: None,
            project_ids: None,
        },
    )
    .await
    .expect("active key creates");
    let revoked = create_api_key(
        app.state::<AppState>(),
        CreateApiKeyInput {
            name: "revoked-key".to_string(),
            permissions: None,
            project_ids: None,
        },
    )
    .await
    .expect("revoked key creates");

    revoke_api_key(
        app.state::<AppState>(),
        RevokeApiKeyInput { id: revoked.id },
    )
    .await
    .expect("key revokes");

    let keys = list_api_keys(app.state::<AppState>())
        .await
        .expect("keys list");
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].id, active.id);
    assert_eq!(keys[0].permissions, 7);
    assert!(keys[0].revoked_at.is_none());
    assert!(keys[0].last_used_at.is_none());
}

// ── revoke_key ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_revoke_api_key_removes_from_active_list() {
    let state = setup_test_state();
    let repo = state.api_key_repo.as_ref();

    let created = ApiKeyService::create_key(repo, "revoke-me", None, &[], KeySource::SettingsUi)
        .await
        .unwrap();

    ApiKeyService::revoke_key(repo, created.key.id.as_str(), KeySource::SettingsUi)
        .await
        .unwrap();

    let keys = repo.list().await.unwrap();
    assert!(
        keys.is_empty(),
        "revoked key must not appear in active list"
    );
}

#[tokio::test]
async fn revoke_api_key_command_removes_key_from_active_list() {
    let app = api_key_command_app();
    let created = create_api_key(
        app.state::<AppState>(),
        CreateApiKeyInput {
            name: "revoke-command".to_string(),
            permissions: None,
            project_ids: None,
        },
    )
    .await
    .expect("key creates");

    revoke_api_key(
        app.state::<AppState>(),
        RevokeApiKeyInput {
            id: created.id.clone(),
        },
    )
    .await
    .expect("key revokes");

    let keys = list_api_keys(app.state::<AppState>())
        .await
        .expect("keys list");
    assert!(keys.is_empty());
}

// ── rotate_key ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_rotate_api_key_returns_new_raw_key() {
    let state = setup_test_state();
    let repo = state.api_key_repo.as_ref();

    let original = ApiKeyService::create_key(repo, "rotate-me", None, &[], KeySource::SettingsUi)
        .await
        .unwrap();

    let rotated = ApiKeyService::rotate_key(repo, original.key.id.as_str(), KeySource::SettingsUi)
        .await
        .unwrap();

    assert!(!rotated.raw_key.is_empty());
    assert_ne!(
        rotated.raw_key, original.raw_key,
        "rotated key must differ from original"
    );
}

#[tokio::test]
async fn rotate_api_key_command_returns_new_key_and_preserves_projects() {
    let app = api_key_command_app();
    let original = create_api_key(
        app.state::<AppState>(),
        CreateApiKeyInput {
            name: "rotate-command".to_string(),
            permissions: Some(3),
            project_ids: Some(vec!["proj-1".to_string()]),
        },
    )
    .await
    .expect("key creates");

    let rotated = rotate_api_key(
        app.state::<AppState>(),
        RotateApiKeyInput {
            id: original.id.clone(),
        },
    )
    .await
    .expect("key rotates");

    assert_ne!(rotated.id, original.id);
    assert_eq!(rotated.name, original.name);
    assert_eq!(rotated.permissions, 3);
    assert_ne!(rotated.raw_key, original.raw_key);

    let keys = list_api_keys(app.state::<AppState>())
        .await
        .expect("keys list");
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].id, rotated.id);
    assert_eq!(keys[0].project_ids, vec!["proj-1"]);
}

#[tokio::test]
async fn rotate_api_key_command_rejects_missing_key() {
    let app = api_key_command_app();

    let error = rotate_api_key(
        app.state::<AppState>(),
        RotateApiKeyInput {
            id: "missing-key".to_string(),
        },
    )
    .await
    .expect_err("missing key should error");

    assert!(error.contains("API key not found: missing-key"));
}

// ── update_permissions ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_update_api_key_permissions() {
    let state = setup_test_state();
    let repo = state.api_key_repo.as_ref();

    let created = ApiKeyService::create_key(repo, "perm-key", None, &[], KeySource::SettingsUi)
        .await
        .unwrap();
    let key_id_str = created.key.id.as_str().to_string();

    // Update to read-only (permissions = 1)
    repo.update_api_key_permissions(&key_id_str, 1)
        .await
        .unwrap();

    let key_id = ApiKeyId::from_string(&key_id_str);
    let updated = repo.get_by_id(&key_id).await.unwrap().unwrap();
    assert_eq!(updated.permissions, 1);
}

#[tokio::test]
async fn update_api_key_permissions_command_updates_active_key_and_logs_audit() {
    let app = api_key_command_app();
    let created = create_api_key(
        app.state::<AppState>(),
        CreateApiKeyInput {
            name: "permissions-command".to_string(),
            permissions: None,
            project_ids: None,
        },
    )
    .await
    .expect("key creates");

    update_api_key_permissions(
        app.state::<AppState>(),
        UpdateApiKeyPermissionsInput {
            id: created.id.clone(),
            permissions: 1,
        },
    )
    .await
    .expect("permissions update");

    let keys = list_api_keys(app.state::<AppState>())
        .await
        .expect("keys list");
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].permissions, 1);

    let audit = get_api_key_audit_log(
        app.state::<AppState>(),
        GetAuditLogInput { id: created.id },
    )
    .await
    .expect("audit log loads");
    assert!(audit.len() >= 2);
}

// ── update_projects ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_update_api_key_projects() {
    let state = setup_test_state();
    let repo = state.api_key_repo.as_ref();

    let created = ApiKeyService::create_key(repo, "proj-key", None, &[], KeySource::SettingsUi)
        .await
        .unwrap();
    let key_id = created.key.id.clone();

    let project_ids = vec!["proj-1".to_string(), "proj-2".to_string()];
    repo.set_projects(&key_id, &project_ids).await.unwrap();

    let stored = repo.get_projects(&key_id).await.unwrap();
    assert_eq!(stored.len(), 2);
    assert!(stored.contains(&"proj-1".to_string()));
    assert!(stored.contains(&"proj-2".to_string()));
}

#[tokio::test]
async fn update_api_key_projects_command_replaces_project_associations() {
    let app = api_key_command_app();
    let created = create_api_key(
        app.state::<AppState>(),
        CreateApiKeyInput {
            name: "projects-command".to_string(),
            permissions: None,
            project_ids: Some(vec!["old-project".to_string()]),
        },
    )
    .await
    .expect("key creates");

    update_api_key_projects(
        app.state::<AppState>(),
        UpdateApiKeyProjectsInput {
            id: created.id.clone(),
            project_ids: vec!["new-a".to_string(), "new-b".to_string()],
        },
    )
    .await
    .expect("projects update");

    let keys = list_api_keys(app.state::<AppState>())
        .await
        .expect("keys list");
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].project_ids, vec!["new-a", "new-b"]);

    let audit = get_api_key_audit_log(
        app.state::<AppState>(),
        GetAuditLogInput { id: created.id },
    )
    .await
    .expect("audit log loads");
    assert!(audit.len() >= 2);
}

// ── audit log ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_audit_log_created_on_operations() {
    let state = setup_test_state();
    let repo = state.api_key_repo.as_ref();

    let created = ApiKeyService::create_key(repo, "audit-key", None, &[], KeySource::SettingsUi)
        .await
        .unwrap();
    let key_id_str = created.key.id.as_str().to_string();

    // create_key logs an audit entry for the creator
    let entries = repo.get_audit_log(&key_id_str, Some(10)).await.unwrap();
    assert!(
        !entries.is_empty(),
        "create_key must log at least one audit entry"
    );
}

#[tokio::test]
async fn get_api_key_audit_log_command_returns_recent_entries() {
    let app = api_key_command_app();
    let created = create_api_key(
        app.state::<AppState>(),
        CreateApiKeyInput {
            name: "audit-command".to_string(),
            permissions: None,
            project_ids: None,
        },
    )
    .await
    .expect("key creates");

    let entries = get_api_key_audit_log(
        app.state::<AppState>(),
        GetAuditLogInput { id: created.id },
    )
    .await
    .expect("audit log loads");

    assert!(!entries.is_empty());
    assert!(entries.iter().any(|entry| entry.success));
}

// ── IPC contract tests ─────────────────────────────────────────────────────────
// Verify camelCase deserialization for input structs and camelCase serialization
// for output structs, matching what the frontend invoke() sends/receives.

#[cfg(test)]
mod ipc_contract {
    use ralphx_lib::commands::api_key_commands::{
        ApiKeyCreatedResponse, ApiKeyInfoResponse, CreateApiKeyInput, GetAuditLogInput,
        RevokeApiKeyInput, RotateApiKeyInput, UpdateApiKeyPermissionsInput,
        UpdateApiKeyProjectsInput,
    };

    // ── Input deserialization ──────────────────────────────────────────────────

    #[test]
    fn create_api_key_input_deserializes_camel_case() {
        let json = r#"{"name":"my-key","permissions":7,"projectIds":["proj-1","proj-2"]}"#;
        let input: CreateApiKeyInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.name, "my-key");
        assert_eq!(input.permissions, Some(7));
        assert_eq!(
            input.project_ids,
            Some(vec!["proj-1".to_string(), "proj-2".to_string()])
        );
    }

    #[test]
    fn create_api_key_input_optional_fields_absent() {
        let json = r#"{"name":"minimal-key"}"#;
        let input: CreateApiKeyInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.name, "minimal-key");
        assert!(input.permissions.is_none());
        assert!(input.project_ids.is_none());
    }

    #[test]
    fn revoke_api_key_input_deserializes_camel_case() {
        let json = r#"{"id":"key-abc-123"}"#;
        let input: RevokeApiKeyInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.id, "key-abc-123");
    }

    #[test]
    fn rotate_api_key_input_deserializes_camel_case() {
        let json = r#"{"id":"key-xyz-456"}"#;
        let input: RotateApiKeyInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.id, "key-xyz-456");
    }

    #[test]
    fn update_api_key_projects_input_deserializes_camel_case() {
        let json = r#"{"id":"key-789","projectIds":["proj-a","proj-b"]}"#;
        let input: UpdateApiKeyProjectsInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.id, "key-789");
        assert_eq!(
            input.project_ids,
            vec!["proj-a".to_string(), "proj-b".to_string()]
        );
    }

    #[test]
    fn update_api_key_projects_input_empty_project_ids() {
        let json = r#"{"id":"key-789","projectIds":[]}"#;
        let input: UpdateApiKeyProjectsInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.id, "key-789");
        assert!(input.project_ids.is_empty());
    }

    #[test]
    fn update_api_key_permissions_input_deserializes_camel_case() {
        let json = r#"{"id":"key-101","permissions":3}"#;
        let input: UpdateApiKeyPermissionsInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.id, "key-101");
        assert_eq!(input.permissions, 3);
    }

    #[test]
    fn get_audit_log_input_deserializes_camel_case() {
        let json = r#"{"id":"key-audit-1"}"#;
        let input: GetAuditLogInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.id, "key-audit-1");
    }

    // ── Output serialization ───────────────────────────────────────────────────

    #[test]
    fn api_key_info_response_serializes_camel_case() {
        let response = ApiKeyInfoResponse {
            id: "key-id-1".to_string(),
            name: "my-api-key".to_string(),
            key_prefix: "rkx_abc".to_string(),
            permissions: 7,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            revoked_at: None,
            last_used_at: Some("2024-06-01T12:00:00Z".to_string()),
            project_ids: vec!["proj-1".to_string()],
        };

        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["id"], "key-id-1");
        assert_eq!(json["name"], "my-api-key");
        assert_eq!(json["keyPrefix"], "rkx_abc");
        assert_eq!(json["permissions"], 7);
        assert_eq!(json["createdAt"], "2024-01-01T00:00:00Z");
        assert!(json["revokedAt"].is_null());
        assert_eq!(json["lastUsedAt"], "2024-06-01T12:00:00Z");
        assert_eq!(json["projectIds"][0], "proj-1");

        // snake_case keys must NOT appear
        assert!(json.get("key_prefix").is_none());
        assert!(json.get("created_at").is_none());
        assert!(json.get("revoked_at").is_none());
        assert!(json.get("last_used_at").is_none());
        assert!(json.get("project_ids").is_none());
    }

    #[test]
    fn api_key_info_response_all_optional_fields_null() {
        let response = ApiKeyInfoResponse {
            id: "key-id-2".to_string(),
            name: "bare-key".to_string(),
            key_prefix: "rkx_xyz".to_string(),
            permissions: 1,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            revoked_at: None,
            last_used_at: None,
            project_ids: vec![],
        };

        let json = serde_json::to_value(&response).unwrap();
        assert!(json["revokedAt"].is_null());
        assert!(json["lastUsedAt"].is_null());
        assert_eq!(json["projectIds"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn api_key_created_response_serializes_camel_case() {
        let response = ApiKeyCreatedResponse {
            id: "new-key-id".to_string(),
            name: "fresh-key".to_string(),
            raw_key: "rkx_supersecretvalue".to_string(),
            key_prefix: "rkx_sup".to_string(),
            permissions: 7,
        };

        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["id"], "new-key-id");
        assert_eq!(json["name"], "fresh-key");
        assert_eq!(json["rawKey"], "rkx_supersecretvalue");
        assert_eq!(json["keyPrefix"], "rkx_sup");
        assert_eq!(json["permissions"], 7);

        // snake_case keys must NOT appear
        assert!(json.get("raw_key").is_none());
        assert!(json.get("key_prefix").is_none());
    }

    #[test]
    fn api_key_created_response_raw_key_roundtrip() {
        // Verify the raw_key field serializes as "rawKey" (camelCase) not "raw_key"
        let response = ApiKeyCreatedResponse {
            id: "id".to_string(),
            name: "n".to_string(),
            raw_key: "secret".to_string(),
            key_prefix: "rkx_se".to_string(),
            permissions: 0,
        };
        let serialized = serde_json::to_string(&response).unwrap();
        assert!(
            serialized.contains("\"rawKey\""),
            "rawKey must be camelCase in JSON output, got: {serialized}"
        );
        assert!(
            !serialized.contains("\"raw_key\""),
            "snake_case raw_key must not appear in JSON output, got: {serialized}"
        );
    }
}
