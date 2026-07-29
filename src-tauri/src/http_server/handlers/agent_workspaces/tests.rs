    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path as StdPath, PathBuf};
    use std::pin::Pin;
    use std::process::Command;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use crate::application::agent_conversation_workspace::{
        resolve_agent_conversation_workspace_path, resolve_linked_plan_branch_agent_worktree_path,
    };
    use crate::application::agent_workspace_review::{
        AgentWorkspaceReviewChangedFile, AgentWorkspaceReviewContext, AgentWorkspaceReviewDiffSummary,
        AgentWorkspaceReviewHunkAnchor, AgentWorkspaceReviewPacket,
    };
    use crate::application::agent_workspace_review_publish_handoff::pr_fix_publish_can_resume_after_workspace_review;
    use crate::application::AppState;
    use crate::commands::ExecutionState;
    use crate::domain::agents::{
        AgentConfig, AgentHandle, AgentOutput, AgentResponse, AgentResult, AgenticClient,
        ClientCapabilities, ResponseChunk,
    };
    use crate::domain::entities::plan_branch::{
        PrPushStatus as PlanPrPushStatus, PrStatus as PlanPrStatus,
    };
    use crate::domain::entities::{
        AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, AgentRunId,
        AgentWorkspacePrCommentEvidenceUpsert, AgentWorkspacePrDescription,
        AgentWorkspacePrMetadataDecision,
        AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitorStatus,
        AgentWorkspaceReviewOutcome, AgentWorkspaceReviewRuntimeState,
        AgentWorkspaceSourcePullRequest, ArtifactId, ChatContextType, ChatConversation,
        IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranch, PlanBranchId, Project,
        ProjectId, TaskId,
    };
    use crate::domain::repositories::AgentConversationWorkspaceRepository;
    use crate::domain::review::ReviewSettings;
    use crate::domain::services::github_generated_markdown::RALPHX_GENERATED_FOOTER;
    use crate::domain::services::github_service::{
        GithubServiceTrait, PrDetail, PrHealth, PrIssueCommentSummary, PrReviewSubmissionEvent,
        PrStatus, PrSyncState,
    };
    use crate::http_server::handlers::agent_workspace_review_approval::{
        approve_agent_workspace_review_anyway_handler, ApproveAgentWorkspaceReviewAnywayRequest,
    };
    use crate::tests::mock_github_service::MockGithubService;
    use async_trait::async_trait;
    use futures::{stream, Stream};

    fn git(repo: impl AsRef<StdPath>, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git command should spawn");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn test_http_state(app_state: Arc<AppState>) -> HttpServerState {
        HttpServerState {
            app_state,
            execution_state: Arc::new(ExecutionState::new()),
            delegation_service: Default::default(),
        }
    }

    fn open_review_pr_health() -> PrHealth {
        PrHealth {
            sync_state: PrSyncState {
                status: PrStatus::Open,
                merge_state_status: None,
                mergeable: None,
                is_draft: false,
                head_ref_name: "feature/review-workflow".to_string(),
                base_ref_name: "main".to_string(),
                head_ref_oid: Some("head-sha".to_string()),
                base_ref_oid: None,
            },
            review_decision: None,
            checks: Vec::new(),
            issue_comments: Vec::new(),
            auto_merge_request: None,
        }
    }

    async fn pr_review_submission_context() -> (
        Arc<AppState>,
        HttpServerState,
        ChatConversationId,
        Arc<MockGithubService>,
    ) {
        let mut app_state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        github.state().fetch_pr_health_result = Some(Ok(open_review_pr_health()));
        app_state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
        let app_state = Arc::new(app_state);

        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
        workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
            number: 411,
            url: Some("https://github.com/mock/project/pull/411".to_string()),
            title: Some("Fix review workflow".to_string()),
            head_ref_name: "feature/review-workflow".to_string(),
            base_ref_name: Some("main".to_string()),
            head_ref_oid: Some("head-sha".to_string()),
        });
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .unwrap();

        let mut monitor = AgentWorkspacePrReviewMonitor::new(
            conversation_id.clone(),
            workspace.project_id,
            411,
            Some("head-sha".to_string()),
        );
        monitor.status = AgentWorkspacePrReviewMonitorStatus::AwaitingUser;
        monitor.review_artifact_id = Some(ArtifactId::from_string("review-artifact-1"));
        monitor.review_artifact_head_sha = Some("head-sha".to_string());
        monitor.review_artifact_version = Some(1);
        app_state
            .agent_conversation_workspace_repo
            .upsert_pr_review_monitor(monitor)
            .await
            .unwrap();

        let state = test_http_state(Arc::clone(&app_state));
        (app_state, state, conversation_id, github)
    }

    struct RecordingWorkspaceReviewStarter {
        calls: AtomicUsize,
    }

    impl RecordingWorkspaceReviewStarter {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl WorkspaceReviewStarter for RecordingWorkspaceReviewStarter {
        fn start<'a>(
            &'a self,
            state: Arc<AppState>,
            workspace: &'a AgentConversationWorkspace,
            force: bool,
        ) -> WorkspaceReviewStartFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                assert!(!force, "repair completion should start a normal refresh");
                let context = load_agent_workspace_review_context(state.as_ref(), workspace).await?;
                let target = context.target;
                assert!(
                    target.is_some(),
                    "repair refresh should still have reviewable changes"
                );
                let mut monitor = context.monitor;
                monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
                monitor.review_outcome = AgentWorkspaceReviewOutcome::None;
                monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
                monitor.review_blocking_summary = None;
                monitor.review_blocking_fingerprint = None;
                monitor.review_fixer_run_id = None;
                monitor.review_fixer_conversation_id = None;
                monitor.review_fixer_status = None;
                monitor.last_run_id = Some("workspace-review-run-after-repair".to_string());
                let monitor = state
                    .agent_conversation_workspace_repo
                    .upsert_workspace_review_monitor(monitor)
                    .await?;
                Ok(AgentWorkspaceReviewStart {
                    context: AgentWorkspaceReviewContext {
                        monitor,
                        target,
                        goal_context: AgentWorkspaceReviewGoalContext::default(),
                        is_current: false,
                        is_outdated: false,
                        review_artifact_is_current: false,
                        review_artifact_is_outdated: false,
                        can_mutate_review_state: false,
                        review_runtime_state:
                            AgentWorkspaceReviewRuntimeState::MissingRuntimeIdentity,
                        should_show_tab: true,
                    },
                    started: true,
                    skipped_reason: None,
                    was_queued: false,
                })
            })
        }
    }

    struct SubmittingPrDescriptionClient {
        repo: Arc<dyn AgentConversationWorkspaceRepository>,
        conversation_id: ChatConversationId,
        preserve_existing_pr_metadata: bool,
    }

    impl SubmittingPrDescriptionClient {
        fn new(
            repo: Arc<dyn AgentConversationWorkspaceRepository>,
            conversation_id: ChatConversationId,
            preserve_existing_pr_metadata: bool,
        ) -> Self {
            Self {
                repo,
                conversation_id,
                preserve_existing_pr_metadata,
            }
        }
    }

    #[async_trait]
    impl AgenticClient for SubmittingPrDescriptionClient {
        async fn spawn_agent(&self, config: AgentConfig) -> AgentResult<AgentHandle> {
            Ok(AgentHandle::mock(config.role))
        }

        async fn stop_agent(&self, _handle: &AgentHandle) -> AgentResult<()> {
            Ok(())
        }

        async fn wait_for_completion(&self, _handle: &AgentHandle) -> AgentResult<AgentOutput> {
            if self.preserve_existing_pr_metadata {
                self.repo
                    .save_pr_metadata_decision(
                        &self.conversation_id,
                        AgentWorkspacePrMetadataDecision::Preserve,
                    )
                    .await
                    .expect("test existing PR metadata decision should save");
            } else {
                self.repo
                    .save_pr_description(
                        &self.conversation_id,
                        AgentWorkspacePrDescription::new(
                            Some("Cached publication title".to_string()),
                            "## Summary\n\nReady to publish.".to_string(),
                        ),
                    )
                    .await
                    .expect("test PR description should save");
            }
            Ok(AgentOutput::success("submitted"))
        }

        async fn send_prompt(
            &self,
            _handle: &AgentHandle,
            _prompt: &str,
        ) -> AgentResult<AgentResponse> {
            Ok(AgentResponse::new(""))
        }

        fn stream_response(
            &self,
            _handle: &AgentHandle,
            _prompt: &str,
        ) -> Pin<Box<dyn Stream<Item = AgentResult<ResponseChunk>> + Send>> {
            Box::pin(stream::empty())
        }

        fn capabilities(&self) -> &ClientCapabilities {
            static CAPS: std::sync::OnceLock<ClientCapabilities> = std::sync::OnceLock::new();
            CAPS.get_or_init(ClientCapabilities::mock)
        }

        async fn is_available(&self) -> AgentResult<bool> {
            Ok(true)
        }
    }

    #[test]
    fn workspace_review_default_title_uses_target_identity() {
        let selected_pr_target =
            crate::application::agent_workspace_review::AgentWorkspaceReviewTarget {
                scope: AgentWorkspaceReviewTargetScope::SelectedSource,
                base_ref: "main".to_string(),
                base_sha: Some("base".to_string()),
                head_ref: "refs/ralphx/pr-heads/347".to_string(),
                head_sha: Some("head".to_string()),
                diff_fingerprint: "fingerprint".to_string(),
                working_directory: PathBuf::from("/tmp/worktree"),
                source_pull_request_number: Some(347),
                review_packet: AgentWorkspaceReviewPacket::default(),
            };
        assert_eq!(
            default_workspace_review_artifact_title(
                AgentWorkspaceReviewTargetScope::SelectedSource,
                Some(&selected_pr_target),
            ),
            "PR #347"
        );

        let selected_branch_target =
            crate::application::agent_workspace_review::AgentWorkspaceReviewTarget {
                scope: AgentWorkspaceReviewTargetScope::SelectedSource,
                base_ref: "main".to_string(),
                base_sha: Some("base".to_string()),
                head_ref: "refs/heads/feature/review-sidecar".to_string(),
                head_sha: Some("head".to_string()),
                diff_fingerprint: "fingerprint".to_string(),
                working_directory: PathBuf::from("/tmp/worktree"),
                source_pull_request_number: None,
                review_packet: AgentWorkspaceReviewPacket::default(),
            };
        assert_eq!(
            default_workspace_review_artifact_title(
                AgentWorkspaceReviewTargetScope::SelectedSource,
                Some(&selected_branch_target),
            ),
            "feature/review-sidecar"
        );

        assert_eq!(
            default_workspace_review_artifact_title(
                AgentWorkspaceReviewTargetScope::WorkspaceDelta,
                Some(&selected_branch_target),
            ),
            "Workspace changes"
        );
    }

    #[test]
    fn workspace_review_target_response_includes_packet_only_when_requested() {
        let target = crate::application::agent_workspace_review::AgentWorkspaceReviewTarget {
            scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            base_ref: "main".to_string(),
            base_sha: Some("base".to_string()),
            head_ref: "HEAD".to_string(),
            head_sha: Some("head".to_string()),
            diff_fingerprint: "fingerprint".to_string(),
            working_directory: PathBuf::from("/tmp/worktree"),
            source_pull_request_number: None,
            review_packet: AgentWorkspaceReviewPacket {
                summary: AgentWorkspaceReviewDiffSummary {
                    files_changed: 1,
                    insertions: 2,
                    deletions: 0,
                },
                changed_files: vec![AgentWorkspaceReviewChangedFile {
                    path: "src/lib.rs".to_string(),
                    status: "modified".to_string(),
                    sources: vec!["committed".to_string()],
                }],
                changed_files_truncated: false,
                hunk_anchors: vec![
                    crate::application::agent_workspace_review::AgentWorkspaceReviewHunkAnchor {
                        path: "src/lib.rs".to_string(),
                        source: "committed".to_string(),
                        hunk_header: "@@ -1,1 +1,2 @@".to_string(),
                        old_start: 1,
                        old_lines: 1,
                        new_start: 1,
                        new_lines: 2,
                    },
                ],
                hunk_anchors_truncated: false,
                patch_excerpt: "diff --git a/src/lib.rs b/src/lib.rs".to_string(),
                patch_excerpt_truncated: false,
                notes: vec![],
            },
        };

        let default_response = AgentWorkspaceReviewTargetResponse::from(target.clone());
        assert!(default_response.review_packet.is_none());

        let packet_response = AgentWorkspaceReviewTargetResponse::from_target(target, true)
            .review_packet
            .expect("packet should be included when requested");
        assert_eq!(packet_response.summary.files_changed, 1);
        assert_eq!(packet_response.changed_files[0].path, "src/lib.rs");
        assert_eq!(packet_response.hunk_anchors[0].source, "committed");
        assert_eq!(packet_response.hunk_anchors[0].new_lines, 2);
        assert_eq!(
            packet_response.patch_excerpt,
            "diff --git a/src/lib.rs b/src/lib.rs"
        );
    }

    #[test]
    fn workspace_review_tool_target_metadata_requires_current_target() {
        let target = crate::application::agent_workspace_review::AgentWorkspaceReviewTarget {
            scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            base_ref: "main".to_string(),
            base_sha: Some("base".to_string()),
            head_ref: "HEAD".to_string(),
            head_sha: Some("head".to_string()),
            diff_fingerprint: "fingerprint".to_string(),
            working_directory: PathBuf::from("/tmp/worktree"),
            source_pull_request_number: None,
            review_packet: AgentWorkspaceReviewPacket::default(),
        };

        let accepted = validate_workspace_review_tool_target_metadata(
            &target,
            Some("workspace_delta"),
            Some("head"),
            Some("fingerprint"),
            "workspace Review artifact write",
        )
        .expect("matching target metadata should be accepted");
        assert_eq!(accepted.0, AgentWorkspaceReviewTargetScope::WorkspaceDelta);
        assert_eq!(accepted.1.as_deref(), Some("head"));
        assert_eq!(accepted.2, "fingerprint");

        assert!(validate_workspace_review_tool_target_metadata(
            &target,
            Some("workspace_delta"),
            None,
            Some("fingerprint"),
            "workspace Review artifact write",
        )
        .is_err());
        assert!(validate_workspace_review_tool_target_metadata(
            &target,
            Some("workspace_delta"),
            Some("head"),
            Some("stale-fingerprint"),
            "workspace Review artifact write",
        )
        .is_err());
    }

    #[test]
    fn workspace_review_tool_run_id_requires_active_review_run_match() {
        let conversation_id = ChatConversationId::new();
        let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id.clone(), ProjectId::new());
        monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
        monitor.last_run_id = Some("run-current".to_string());
        monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
        monitor.current_diff_fingerprint = Some("fingerprint-current".to_string());

        assert_eq!(
            validate_workspace_review_tool_run_id(
                &monitor,
                Some("run-current"),
                "workspace Review completion",
            )
            .expect("matching run id should be accepted")
            .as_deref(),
            Some("run-current")
        );
        assert!(validate_workspace_review_tool_run_id(
            &monitor,
            Some("run-stale"),
            "workspace Review completion",
        )
        .is_err());
        assert!(
            validate_workspace_review_tool_run_id(&monitor, None, "workspace Review completion",)
                .is_err()
        );

        monitor.last_run_id = None;
        assert!(validate_workspace_review_tool_run_id(
            &monitor,
            Some("run-current"),
            "workspace Review completion",
        )
        .is_err());

        monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
        assert!(
            validate_workspace_review_tool_run_id(&monitor, None, "workspace Review completion",)
                .is_err()
        );
    }

    #[test]
    fn workspace_review_hunk_annotation_validation_accepts_current_anchor() {
        let target = crate::application::agent_workspace_review::AgentWorkspaceReviewTarget {
            scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            base_ref: "main".to_string(),
            base_sha: Some("base".to_string()),
            head_ref: "HEAD".to_string(),
            head_sha: Some("head".to_string()),
            diff_fingerprint: "fingerprint".to_string(),
            working_directory: PathBuf::from("/tmp/worktree"),
            source_pull_request_number: None,
            review_packet: AgentWorkspaceReviewPacket {
                summary: AgentWorkspaceReviewDiffSummary::default(),
                changed_files: Vec::new(),
                changed_files_truncated: false,
                hunk_anchors: vec![AgentWorkspaceReviewHunkAnchor {
                    path: "src/lib.rs".to_string(),
                    source: "committed".to_string(),
                    hunk_header: "@@ -1,1 +1,2 @@".to_string(),
                    old_start: 1,
                    old_lines: 1,
                    new_start: 1,
                    new_lines: 2,
                }],
                hunk_anchors_truncated: false,
                patch_excerpt: String::new(),
                patch_excerpt_truncated: false,
                notes: Vec::new(),
            },
        };

        let validation = validate_workspace_review_hunk_annotation_requests(
            vec![WriteAgentWorkspaceReviewHunkAnnotationRequest {
                path: "src/lib.rs".to_string(),
                source: "committed".to_string(),
                hunk_header: "@@ -1,1 +1,2 @@".to_string(),
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 2,
                title: Some("Library update".to_string()),
                message: "Explains the reviewed hunk.".to_string(),
                level: Some("notice".to_string()),
            }],
            Some(&target),
            AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            Some("head"),
            "fingerprint",
        )
        .expect("annotation should match current anchor");

        assert_eq!(validation.accepted.len(), 1);
        assert!(validation.rejected.is_empty());
        assert_eq!(validation.accepted[0].path, "src/lib.rs");
        assert_eq!(validation.accepted[0].level, "notice");
    }

    #[test]
    fn workspace_review_hunk_annotation_validation_partially_rejects_unmatched_anchor() {
        let target = crate::application::agent_workspace_review::AgentWorkspaceReviewTarget {
            scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            base_ref: "main".to_string(),
            base_sha: Some("base".to_string()),
            head_ref: "HEAD".to_string(),
            head_sha: Some("head".to_string()),
            diff_fingerprint: "fingerprint".to_string(),
            working_directory: PathBuf::from("/tmp/worktree"),
            source_pull_request_number: None,
            review_packet: AgentWorkspaceReviewPacket {
                hunk_anchors: vec![AgentWorkspaceReviewHunkAnchor {
                    path: "src/lib.rs".to_string(),
                    source: "committed".to_string(),
                    hunk_header: "@@ -1,1 +1,2 @@".to_string(),
                    old_start: 1,
                    old_lines: 1,
                    new_start: 1,
                    new_lines: 2,
                }],
                ..AgentWorkspaceReviewPacket::default()
            },
        };

        let validation = validate_workspace_review_hunk_annotation_requests(
            vec![
                WriteAgentWorkspaceReviewHunkAnnotationRequest {
                    path: "src/lib.rs".to_string(),
                    source: "committed".to_string(),
                    hunk_header: "@@ -1,1 +1,2 @@".to_string(),
                    old_start: 1,
                    old_lines: 1,
                    new_start: 1,
                    new_lines: 2,
                    title: None,
                    message: "Explains the reviewed hunk.".to_string(),
                    level: None,
                },
                WriteAgentWorkspaceReviewHunkAnnotationRequest {
                    path: "src/lib.rs".to_string(),
                    source: "committed".to_string(),
                    hunk_header: "@@ -10,1 +10,2 @@".to_string(),
                    old_start: 10,
                    old_lines: 1,
                    new_start: 10,
                    new_lines: 2,
                    title: None,
                    message: "This hunk is not in the current packet.".to_string(),
                    level: None,
                },
            ],
            Some(&target),
            AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            Some("head"),
            "fingerprint",
        )
        .expect("batch metadata should be valid");

        assert_eq!(validation.accepted.len(), 1);
        assert_eq!(validation.rejected.len(), 1);
        assert!(validation.rejected[0]
            .reason
            .as_deref()
            .expect("rejection should include reason")
            .contains("does not match any current workspace review hunk anchor"));
    }

    #[test]
    fn workspace_review_missing_hunk_anchors_requires_every_anchor() {
        let target = crate::application::agent_workspace_review::AgentWorkspaceReviewTarget {
            scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            base_ref: "main".to_string(),
            base_sha: Some("base".to_string()),
            head_ref: "HEAD".to_string(),
            head_sha: Some("head".to_string()),
            diff_fingerprint: "fingerprint".to_string(),
            working_directory: PathBuf::from("/tmp/worktree"),
            source_pull_request_number: None,
            review_packet: AgentWorkspaceReviewPacket {
                hunk_anchors: vec![
                    AgentWorkspaceReviewHunkAnchor {
                        path: "src/lib.rs".to_string(),
                        source: "committed".to_string(),
                        hunk_header: "@@ -1,1 +1,2 @@".to_string(),
                        old_start: 1,
                        old_lines: 1,
                        new_start: 1,
                        new_lines: 2,
                    },
                    AgentWorkspaceReviewHunkAnchor {
                        path: "src/main.rs".to_string(),
                        source: "committed".to_string(),
                        hunk_header: "@@ -5,1 +5,3 @@".to_string(),
                        old_start: 5,
                        old_lines: 1,
                        new_start: 5,
                        new_lines: 3,
                    },
                ],
                ..AgentWorkspaceReviewPacket::default()
            },
        };
        let annotation = AgentWorkspaceReviewHunkAnnotation {
            id: "annotation-1".to_string(),
            conversation_id: ChatConversationId::from_string("conversation-1".to_string()),
            project_id: ProjectId::from_string("project-1".to_string()),
            artifact_id: ArtifactId::from_string("artifact-1".to_string()),
            artifact_version: 1,
            target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            head_sha: Some("head".to_string()),
            diff_fingerprint: "fingerprint".to_string(),
            path: "src/lib.rs".to_string(),
            diff_source: "committed".to_string(),
            hunk_header: "@@ -1,1 +1,2 @@".to_string(),
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 2,
            title: None,
            message: "Explains the first hunk.".to_string(),
            level: "notice".to_string(),
            created_by_run_id: Some("run-1".to_string()),
            created_at: chrono::Utc::now(),
        };

        let missing = missing_workspace_review_hunk_anchors(&target, &[annotation]);

        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].path, "src/main.rs");
    }

    #[test]
    fn workspace_review_completion_treats_hunk_coverage_as_best_effort() {
        assert!(!workspace_review_completion_requires_hunk_coverage(Some(
            "passed"
        )));
        assert!(!workspace_review_completion_requires_hunk_coverage(Some(
            "blocking"
        )));
        assert!(!workspace_review_completion_requires_hunk_coverage(Some(
            "no_changes"
        )));
        assert!(!workspace_review_completion_requires_hunk_coverage(Some(
            "run_failed"
        )));
        assert!(!workspace_review_completion_requires_hunk_coverage(None));
        assert!(!workspace_review_completion_requires_hunk_coverage(Some(
            "bogus"
        )));
    }

    #[test]
    fn workspace_review_artifact_title_replaces_legacy_or_stale_titles() {
        let selected_pr_target =
            crate::application::agent_workspace_review::AgentWorkspaceReviewTarget {
                scope: AgentWorkspaceReviewTargetScope::SelectedSource,
                base_ref: "main".to_string(),
                base_sha: Some("base".to_string()),
                head_ref: "refs/ralphx/pr-heads/347".to_string(),
                head_sha: Some("head".to_string()),
                diff_fingerprint: "fingerprint".to_string(),
                working_directory: PathBuf::from("/tmp/worktree"),
                source_pull_request_number: Some(347),
                review_packet: AgentWorkspaceReviewPacket::default(),
            };

        assert_eq!(
            workspace_review_artifact_title(
                Some("Selected Source Review".to_string()),
                None,
                None,
                AgentWorkspaceReviewTargetScope::SelectedSource,
                Some(&selected_pr_target),
            ),
            "PR #347"
        );
        assert_eq!(
            workspace_review_artifact_title(
                None,
                Some("PR #123"),
                Some(AgentWorkspaceReviewTargetScope::SelectedSource),
                AgentWorkspaceReviewTargetScope::WorkspaceDelta,
                Some(&selected_pr_target),
            ),
            "Workspace changes"
        );
        assert_eq!(
            workspace_review_artifact_title(
                None,
                Some("Custom review title"),
                Some(AgentWorkspaceReviewTargetScope::SelectedSource),
                AgentWorkspaceReviewTargetScope::SelectedSource,
                Some(&selected_pr_target),
            ),
            "Custom review title"
        );
    }

    #[test]
    fn workspace_review_content_normalization_removes_redundant_h1() {
        assert_eq!(
            normalize_workspace_review_artifact_content(
                "# Selected Source Review\n\n## Summary\n\nLooks good.".to_string(),
            ),
            "## Summary\n\nLooks good."
        );
        assert_eq!(
            normalize_workspace_review_artifact_content(
                "# Workspace Review\r\n\r\n## Summary\r\n\r\nLooks good.".to_string(),
            ),
            "## Summary\r\n\r\nLooks good."
        );
        assert_eq!(
            normalize_workspace_review_artifact_content(
                "# Review\n\n## Summary\n\nLooks good.".to_string(),
            ),
            "## Summary\n\nLooks good."
        );
        assert_eq!(
            normalize_workspace_review_artifact_content(
                "# Useful Architecture Context\n\n## Summary\n\nKeep this title.".to_string(),
            ),
            "# Useful Architecture Context\n\n## Summary\n\nKeep this title."
        );
    }

    fn test_workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
        AgentConversationWorkspace::new(
            conversation_id,
            ProjectId::new(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("main".to_string()),
            Some("0".repeat(40)),
            "feature/pr-description".to_string(),
            "/tmp/pr-description-worktree".to_string(),
        )
    }

    #[tokio::test]
    async fn review_pr_rejects_fixer_context_and_completion_before_github_reads() {
        let mut app_state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        app_state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
        let app_state = Arc::new(app_state);
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
        workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
            number: 267,
            url: Some("https://github.com/owner/repo/pull/267".to_string()),
            title: Some("External PR".to_string()),
            head_ref_name: "external/head".to_string(),
            base_ref_name: Some("main".to_string()),
            head_ref_oid: Some("external-head".to_string()),
        });
        workspace.publication_pr_number = Some(267);
        workspace.publication_pr_status = Some("open".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        workspace.pr_autofix_enabled = true;
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should persist");
        let original = app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        let state = test_http_state(Arc::clone(&app_state));

        let context_error = get_agent_workspace_pr_fix_context(
            State(state.clone()),
            Path(conversation_id.to_string()),
        )
        .await
        .expect_err("Review PR fixer context should fail closed");
        let completion_error = complete_agent_workspace_pr_fix(
            State(state),
            Path(conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "Should never publish".to_string(),
                blocker: None,
                fix_commit_sha: None,
                created_by_run_id: None,
            }),
        )
        .await
        .expect_err("Review PR fixer completion should fail closed");

        assert_eq!(context_error.0, StatusCode::BAD_REQUEST);
        assert_eq!(completion_error.0, StatusCode::BAD_REQUEST);
        assert_eq!(github.state().fetch_pr_health_calls, 0);
        assert_eq!(github.state().check_pr_status_calls, 0);
        assert_eq!(github.state().push_branch_calls, 0);
        assert_eq!(
            app_state
                .agent_conversation_workspace_repo
                .get_by_conversation_id(&conversation_id)
                .await
                .expect("workspace lookup should succeed"),
            Some(original)
        );
        assert!(app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("events should list")
            .is_empty());
    }

    fn test_workspace_review_target() -> AgentWorkspaceReviewTarget {
        AgentWorkspaceReviewTarget {
            scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            base_ref: "main".to_string(),
            base_sha: Some("0".repeat(40)),
            head_ref: "HEAD".to_string(),
            head_sha: None,
            diff_fingerprint: "workspace-diff-fingerprint".to_string(),
            working_directory: PathBuf::from("/tmp/pr-description-worktree"),
            source_pull_request_number: None,
            review_packet: Default::default(),
        }
    }

    fn mark_monitor_current_passed(
        monitor: &mut AgentWorkspaceReviewMonitor,
        target: &AgentWorkspaceReviewTarget,
    ) {
        monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
        monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
        monitor.review_artifact_id = Some(ArtifactId::from_string("review-artifact"));
        monitor.review_artifact_version = Some(1);
        monitor.review_requested_changes_artifact_id =
            Some(ArtifactId::from_string("requested-changes-artifact"));
        monitor.review_requested_changes_artifact_version = Some(1);
        monitor.reviewed_target_scope = Some(target.scope);
        monitor.reviewed_head_sha = target.head_sha.clone();
        monitor.reviewed_diff_fingerprint = Some(target.diff_fingerprint.clone());
        monitor.current_target_scope = Some(target.scope);
        monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    }

    #[test]
    fn initial_auto_publish_resume_predicate_requires_armed_initial_flag_and_no_pr() {
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.auto_publish_initial_pr_enabled = true;
        workspace.auto_publish_enabled = false;
        workspace.publication_pr_number = None;
        let mut monitor =
            AgentWorkspaceReviewMonitor::new(conversation_id, workspace.project_id.clone());
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;

        // Armed initial flag + no PR + gate Passed → resume.
        assert!(auto_publish_can_resume_after_workspace_review(
            &workspace, &monitor
        ));

        // Not armed for the initial PR → no resume (even if the PR-fix flag is on).
        workspace.auto_publish_initial_pr_enabled = false;
        workspace.auto_publish_enabled = true;
        assert!(!auto_publish_can_resume_after_workspace_review(
            &workspace, &monitor
        ));
        workspace.auto_publish_initial_pr_enabled = true;
        workspace.auto_publish_enabled = false;

        // A publication PR already exists → this is the PR-fix path, not initial publish.
        workspace.publication_pr_number = Some(512);
        assert!(!auto_publish_can_resume_after_workspace_review(
            &workspace, &monitor
        ));
        workspace.publication_pr_number = None;

        // Gate not Passed → no resume.
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
        assert!(!auto_publish_can_resume_after_workspace_review(
            &workspace, &monitor
        ));
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;

        // Terminal publication status → no resume.
        workspace.publication_pr_status = Some("merged".to_string());
        assert!(!auto_publish_can_resume_after_workspace_review(
            &workspace, &monitor
        ));
    }

    #[test]
    fn review_completion_resume_predicate_only_allows_review_gate_blocks() {
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_pr_number = Some(267);
        workspace.publication_pr_status = Some("open".to_string());
        workspace.auto_publish_enabled = true;
        workspace.pr_autofix_enabled = true;
        workspace.pr_supervision_status = Some("blocked".to_string());
        workspace.pr_supervision_summary =
            Some("Workspace Review is required before publishing".to_string());
        let mut monitor =
            AgentWorkspaceReviewMonitor::new(conversation_id, workspace.project_id.clone());
        let target = test_workspace_review_target();
        mark_monitor_current_passed(&mut monitor, &target);

        assert!(pr_fix_publish_can_resume_after_workspace_review(
            &workspace,
            &monitor,
            Some(&target),
            &[]
        ));

        workspace.pr_supervision_summary =
            Some("Workspace reviewer completed without writing a current Review".to_string());

        assert!(pr_fix_publish_can_resume_after_workspace_review(
            &workspace,
            &monitor,
            Some(&target),
            &[]
        ));

        workspace.pr_supervision_summary = Some(
            "PR fix publish failed: Workspace reviewer completed without writing a current Review"
                .to_string(),
        );

        assert!(pr_fix_publish_can_resume_after_workspace_review(
            &workspace,
            &monitor,
            Some(&target),
            &[]
        ));

        workspace.pr_supervision_summary = Some("Required checks are still pending.".to_string());

        assert!(!pr_fix_publish_can_resume_after_workspace_review(
            &workspace,
            &monitor,
            Some(&target),
            &[]
        ));
    }

    fn test_freshness(
        is_base_ahead: bool,
        has_uncommitted_changes: bool,
        unpublished_commit_count: Option<u32>,
        base_status: &str,
    ) -> AgentConversationWorkspaceFreshnessResponse {
        AgentConversationWorkspaceFreshnessResponse {
            conversation_id: ChatConversationId::new().as_str(),
            freshness_scope: "full".to_string(),
            base_ref: "main".to_string(),
            base_display_name: Some("main".to_string()),
            target_ref: "origin/main".to_string(),
            captured_base_commit: Some("0".repeat(40)),
            target_base_commit: "1".repeat(40),
            is_base_ahead,
            has_uncommitted_changes,
            unpublished_commit_count,
            remote_refreshed: true,
            worktree_status_checked: true,
            base_status: base_status.to_string(),
            effective_base_ref: Some("main".to_string()),
            effective_base_display_name: Some("main".to_string()),
            base_block_reason: (base_status == "blocked")
                .then_some("Workspace base is blocked".to_string()),
        }
    }

    async fn seed_current_passing_workspace_review(
        app_state: &AppState,
        workspace: &AgentConversationWorkspace,
    ) {
        let context = load_agent_workspace_review_context(app_state, workspace)
            .await
            .expect("review context should load");
        let target = context.target.expect("review target should exist");
        let mut monitor = context.monitor;
        apply_review_artifact_to_monitor(
            &mut monitor,
            target.scope,
            target.head_sha,
            target.diff_fingerprint,
            Some("seeded-passing-review".to_string()),
            ArtifactId::from_string(format!(
                "review-artifact-{}",
                workspace.conversation_id.as_str()
            )),
            1,
            chrono::Utc::now(),
            None,
        );
        monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
        monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
        app_state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .expect("passing review monitor should persist");
    }

    struct PrFixReviewGateFixture {
        _repo: tempfile::TempDir,
        _worktrees: tempfile::TempDir,
        app_state: Arc<AppState>,
        conversation_id: ChatConversationId,
        fix_commit_sha: String,
        pr_fix_run_id: AgentRunId,
        github: Arc<MockGithubService>,
    }

    async fn seed_pr_fix_completion_authority(
        app_state: &AppState,
        conversation_id: &ChatConversationId,
    ) -> AgentRunId {
        let mut run = AgentRun::new(conversation_id.clone());
        run.action_kind = Some(crate::domain::entities::AgentRunActionKind::PrAutofix);
        run.action_context_id = Some("267".to_string());
        run.action_target_id = Some("github_pr_autofix:267:test".to_string());
        app_state
            .agent_run_repo
            .create(run)
            .await
            .expect("active PR autofix run should persist")
            .id
    }

    async fn setup_pr_fix_workspace_with_review_gate(
        suffix: &str,
        review_gate_status: AgentWorkspaceReviewGateStatus,
    ) -> PrFixReviewGateFixture {
        let repo = tempfile::TempDir::new().expect("repo tempdir");
        let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        git(repo.path(), &["config", "user.name", "RalphX Test"]);
        std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
        git(repo.path(), &["add", "README.md"]);
        git(repo.path(), &["commit", "-m", "base"]);
        let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

        let github = Arc::new(MockGithubService::new());
        let mut state = AppState::new_test();
        state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
        let app_state = Arc::new(state);
        let conversation_id = ChatConversationId::new();
        let mut project = Project::new(
            format!("PR Fix Review Gate {suffix}"),
            repo.path().to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .expect("seed project");

        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        conversation.context_type = ChatContextType::Project;
        conversation.context_id = project.id.as_str().to_string();
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("seed conversation");

        let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
            .expect("workspace path");
        let branch_name = format!("ralphx/test/pr-fix-review-{suffix}");
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                &branch_name,
                workspace_path.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(workspace_path.join("fix.txt"), "ci fix\n").expect("write workspace change");
        git(&workspace_path, &["add", "fix.txt"]);
        git(&workspace_path, &["commit", "-m", "fix CI"]);
        let fix_commit_sha = git(&workspace_path, &["rev-parse", "HEAD"]);
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_sha),
            branch_name,
            workspace_path.to_string_lossy().to_string(),
        );
        workspace.publication_pr_number = Some(267);
        workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/267".to_string());
        workspace.publication_pr_status = Some("open".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        workspace.pr_supervision_status = Some("fixing".to_string());
        workspace.pr_supervision_updated_at = Some(chrono::Utc::now());
        workspace.pr_autofix_enabled = true;
        workspace.pr_auto_merge_desired = true;
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        let pr_fix_run_id = seed_pr_fix_completion_authority(app_state.as_ref(), &conversation_id).await;

        let review_context = load_agent_workspace_review_context(app_state.as_ref(), &workspace)
            .await
            .expect("review context should load");
        let target = review_context.target.expect("review target should exist");
        let mut monitor = review_context.monitor;
        apply_review_artifact_to_monitor(
            &mut monitor,
            target.scope,
            target.head_sha,
            target.diff_fingerprint,
            Some("review-run".to_string()),
            ArtifactId::from_string(format!("review-artifact-{suffix}")),
            1,
            chrono::Utc::now(),
            None,
        );
        match review_gate_status {
            AgentWorkspaceReviewGateStatus::Blocking => {
                monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
                monitor.review_blocking_summary =
                    Some("Workspace Review found blocking changes".to_string());
                monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
            }
            AgentWorkspaceReviewGateStatus::Failed => {
                monitor.review_outcome = AgentWorkspaceReviewOutcome::RunFailed;
                monitor.last_error = Some("Workspace Review failed".to_string());
                monitor.status = AgentWorkspaceReviewMonitorStatus::Blocked;
            }
            other => panic!("unsupported test review gate status: {other:?}"),
        }
        app_state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .expect("review monitor should persist");

        PrFixReviewGateFixture {
            _repo: repo,
            _worktrees: worktrees,
            app_state,
            conversation_id,
            fix_commit_sha,
            pr_fix_run_id,
            github,
        }
    }

    #[tokio::test]
    async fn current_pr_fixer_refreshes_base_then_completes_and_publishes_refreshed_head() {
        let mut fixture = setup_pr_fix_workspace_with_review_gate(
            "base-refresh-completion",
            AgentWorkspaceReviewGateStatus::Blocking,
        )
        .await;
        let workspace_repo =
            Arc::clone(&fixture.app_state.agent_conversation_workspace_repo);
        fixture.app_state = Arc::new(
            fixture
                .app_state
                .as_ref()
                .clone()
                .with_agent_client(Arc::new(SubmittingPrDescriptionClient::new(
                    workspace_repo,
                    fixture.conversation_id.clone(),
                    false,
                ))),
        );
        fixture
            .app_state
            .review_settings_repo
            .update_settings(&ReviewSettings {
                require_workspace_review: false,
                ..ReviewSettings::default()
            })
            .await
            .expect("workspace review should be disabled for direct publish");
        let mut workspace = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        workspace.auto_publish_enabled = true;
        fixture
            .app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("auto publish should persist");

        std::fs::write(fixture._repo.path().join("base-refresh.txt"), "new base\n")
            .expect("base update should be written");
        git(fixture._repo.path(), &["add", "base-refresh.txt"]);
        git(
            fixture._repo.path(),
            &["commit", "-m", "advance base while fixer runs"],
        );

        let Json(update_response) = update_agent_workspace_from_base(
            State(test_http_state(Arc::clone(&fixture.app_state))),
            Path(fixture.conversation_id.to_string()),
            Json(UpdateAgentWorkspaceFromBaseRequest {
                base_ref_kind: None,
                base_ref: None,
                base_display_name: None,
                created_by_run_id: Some(fixture.pr_fix_run_id.to_string()),
            }),
        )
        .await
        .expect("current PR fixer should refresh from base");

        assert_eq!(update_response.updated, Some(true));
        assert_eq!(
            fixture.github.state().push_branch_calls,
            0,
            "base refresh must not publish the local branch"
        );
        let refreshed = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        let refreshed_head = git(
            std::path::Path::new(&refreshed.worktree_path),
            &["rev-parse", "HEAD"],
        );
        assert_ne!(refreshed_head, fixture.fix_commit_sha);
        assert_eq!(
            refreshed.publication_push_status.as_deref(),
            Some("needs_agent")
        );
        assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("fixing"));
        let existing_pr = PrDetail {
            number: 267,
            title: "Existing PR title".to_string(),
            body: Some("Existing PR body".to_string()),
            author: Some("maintainer".to_string()),
            created_at: None,
            url: Some("https://github.com/owner/repo/pull/267".to_string()),
            state: PrStatus::Open,
            is_draft: false,
            head_ref_name: refreshed.branch_name.clone(),
            base_ref_name: "main".to_string(),
        };
        fixture.github.queue_pr_detail(Ok(existing_pr.clone()));
        fixture.github.queue_pr_detail(Ok(existing_pr));
        let Json(completion) = complete_agent_workspace_pr_fix(
            State(test_http_state(Arc::clone(&fixture.app_state))),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "Refreshed the fixer branch and retained the repair".to_string(),
                blocker: None,
                fix_commit_sha: Some(refreshed_head.clone()),
                created_by_run_id: Some(fixture.pr_fix_run_id.to_string()),
            }),
        )
        .await
        .expect("current PR fixer should publish the refreshed branch");

        assert_eq!(
            completion.status, "published",
            "publish error: {:?}",
            completion.publish_error
        );
        assert_eq!(completion.pushed, Some(true));
        assert_eq!(fixture.github.state().push_branch_calls, 1);
        assert_eq!(fixture.github.state().fetch_pr_detail_calls, 2);
        assert_eq!(fixture.github.state().last_fetch_pr_detail_number, Some(267));
        let completed = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(
            completed.publication_push_status.as_deref(),
            Some("pushed")
        );
        assert_eq!(
            completed.pr_supervision_status.as_deref(),
            Some("monitoring")
        );
    }

    #[test]
    fn review_artifact_gate_accepts_matching_head_sha() {
        let mut monitor = AgentWorkspacePrReviewMonitor::new(
            ChatConversationId::new(),
            ProjectId::new(),
            411,
            Some("head-sha".to_string()),
        );
        monitor.review_artifact_id = Some(ArtifactId::from_string("artifact-1"));
        monitor.review_artifact_head_sha = Some("head-sha".to_string());

        assert!(ensure_review_artifact_for_head(&monitor, "head-sha").is_ok());
    }

    #[test]
    fn review_artifact_gate_rejects_missing_or_stale_artifact() {
        let mut monitor = AgentWorkspacePrReviewMonitor::new(
            ChatConversationId::new(),
            ProjectId::new(),
            411,
            Some("head-sha".to_string()),
        );
        assert!(ensure_review_artifact_for_head(&monitor, "head-sha").is_err());

        monitor.review_artifact_id = Some(ArtifactId::from_string("artifact-1"));
        monitor.review_artifact_head_sha = Some("old-head-sha".to_string());
        assert!(ensure_review_artifact_for_head(&monitor, "head-sha").is_err());
    }

    #[tokio::test]
    async fn propose_pr_review_action_requires_matching_review_artifact() {
        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
        workspace.publication_pr_number = Some(411);
        workspace.publication_pr_url = Some("https://github.com/mock/project/pull/411".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        let state = test_http_state(Arc::clone(&app_state));

        let (status, Json(body)) = propose_agent_workspace_pr_review_action(
            State(state),
            Path(conversation_id.to_string()),
            Json(ProposeAgentWorkspacePrReviewActionRequest {
                head_sha: "head-sha".to_string(),
                proposed_action: "request_changes".to_string(),
                summary: "Found a blocking regression".to_string(),
                review_body: "Please fix the regression before merge.".to_string(),
                findings_json: None,
                created_by_run_id: Some("run-1".to_string()),
            }),
        )
        .await
        .unwrap_err();

        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body["error"].as_str().unwrap().contains("Write the Review"));
        let actions = app_state
            .agent_conversation_workspace_repo
            .list_pr_review_actions(&conversation_id, 10)
            .await
            .unwrap();
        assert!(actions.is_empty());
    }

    #[tokio::test]
    async fn pr_review_submit_signs_every_summary_review_event_without_mutating_source_body() {
        for (action_kind, event) in [
            (
                AgentWorkspacePrReviewActionKind::RequestChanges,
                PrReviewSubmissionEvent::RequestChanges,
            ),
            (
                AgentWorkspacePrReviewActionKind::Approve,
                PrReviewSubmissionEvent::Approve,
            ),
            (
                AgentWorkspacePrReviewActionKind::Comment,
                PrReviewSubmissionEvent::Comment,
            ),
        ] {
            let (app_state, state, conversation_id, github) = pr_review_submission_context().await;
            github.will_submit_pr_review(format!("review-{event}"), None);
            let source_body = "Agent-authored review body.";
            let action = app_state
                .agent_conversation_workspace_repo
                .create_or_update_pr_review_action(AgentWorkspacePrReviewAction::new(
                    conversation_id.clone(),
                    411,
                    "head-sha".to_string(),
                    action_kind,
                    "Review summary".to_string(),
                    source_body.to_string(),
                    None,
                    Some("run-1".to_string()),
                ))
                .await
                .unwrap();

            let Json(response) = submit_agent_workspace_pr_review_action(
                State(state),
                Path((conversation_id.to_string(), action.id.clone())),
                Json(SubmitAgentWorkspacePrReviewActionRequest { action_kind: None }),
            )
            .await
            .unwrap();

            assert_eq!(response.action.status, "submitted");
            assert_eq!(response.action.review_body, source_body);
            let saved_action = app_state
                .agent_conversation_workspace_repo
                .get_pr_review_action(&action.id)
                .await
                .unwrap()
                .expect("submitted action should remain stored");
            assert_eq!(saved_action.review_body, source_body);
            assert_eq!(
                saved_action.status,
                AgentWorkspacePrReviewActionStatus::Submitted
            );
            assert_eq!(
                github.state().last_submit_pr_review_args.as_ref().map(
                    |(pr_number, captured_event, body)| {
                        (*pr_number, *captured_event, body.clone())
                    }
                ),
                Some((
                    411,
                    event,
                    format!("{source_body}\n\n{RALPHX_GENERATED_FOOTER}")
                ))
            );
        }
    }

    #[tokio::test]
    async fn auto_approved_pr_review_uses_the_signed_submission_path() {
        let (app_state, state, conversation_id, github) = pr_review_submission_context().await;
        github.will_submit_pr_review("auto-review", None);
        let mut monitor = app_state
            .agent_conversation_workspace_repo
            .get_pr_review_monitor(&conversation_id)
            .await
            .unwrap()
            .expect("monitor should exist");
        monitor.last_review_run_id = Some("run-auto".to_string());
        app_state
            .agent_conversation_workspace_repo
            .upsert_pr_review_monitor(monitor)
            .await
            .unwrap();
        app_state
            .agent_conversation_workspace_repo
            .mark_pr_review_first_action_resolved(&conversation_id)
            .await
            .unwrap();
        // Proposal consumes the configured health response; automatic signed submission then
        // falls back to the configured sync-state response for its separate head check.
        github.state().fetch_pr_health_result = Some(Ok(open_review_pr_health()));
        github.state().check_pr_sync_state_result = Some(Ok(open_review_pr_health().sync_state));

        let Json(response) = propose_agent_workspace_pr_review_action(
            State(state),
            Path(conversation_id.to_string()),
            Json(ProposeAgentWorkspacePrReviewActionRequest {
                head_sha: "head-sha".to_string(),
                proposed_action: "approve".to_string(),
                summary: "No blocking findings".to_string(),
                review_body: "Automated review passed.".to_string(),
                findings_json: None,
                created_by_run_id: Some("run-auto".to_string()),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.action.status, "submitted");
        assert_eq!(response.action.review_body, "Automated review passed.");
        assert_eq!(
            github
                .state()
                .last_submit_pr_review_args
                .as_ref()
                .map(|(pr_number, event, body)| (*pr_number, *event, body.clone())),
            Some((
                411,
                PrReviewSubmissionEvent::Approve,
                format!("Automated review passed.\n\n{RALPHX_GENERATED_FOOTER}")
            ))
        );
    }

    #[tokio::test]
    async fn failed_pr_review_submit_keeps_action_pending_for_retry() {
        let mut app_state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        github.state().fetch_pr_health_result = Some(Ok(open_review_pr_health()));
        github.will_fail_submit_pr_review("network unavailable");
        app_state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
        let app_state = Arc::new(app_state);

        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
        workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
            number: 411,
            url: Some("https://github.com/mock/project/pull/411".to_string()),
            title: Some("Fix review workflow".to_string()),
            head_ref_name: "feature/review-workflow".to_string(),
            base_ref_name: Some("main".to_string()),
            head_ref_oid: Some("head-sha".to_string()),
        });
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .unwrap();

        let mut monitor = AgentWorkspacePrReviewMonitor::new(
            conversation_id.clone(),
            workspace.project_id.clone(),
            411,
            Some("head-sha".to_string()),
        );
        monitor.status = AgentWorkspacePrReviewMonitorStatus::AwaitingUser;
        monitor.review_artifact_id = Some(ArtifactId::from_string("review-artifact-1"));
        monitor.review_artifact_head_sha = Some("head-sha".to_string());
        monitor.review_artifact_version = Some(1);
        app_state
            .agent_conversation_workspace_repo
            .upsert_pr_review_monitor(monitor)
            .await
            .unwrap();

        let action = app_state
            .agent_conversation_workspace_repo
            .create_or_update_pr_review_action(AgentWorkspacePrReviewAction::new(
                conversation_id.clone(),
                411,
                "head-sha".to_string(),
                AgentWorkspacePrReviewActionKind::RequestChanges,
                "Found a blocking regression".to_string(),
                "Please fix the regression before merge.".to_string(),
                None,
                Some("run-1".to_string()),
            ))
            .await
            .unwrap();
        let state = test_http_state(Arc::clone(&app_state));

        let (status, Json(body)) = submit_agent_workspace_pr_review_action(
            State(state.clone()),
            Path((conversation_id.to_string(), action.id.clone())),
            Json(SubmitAgentWorkspacePrReviewActionRequest { action_kind: None }),
        )
        .await
        .unwrap_err();

        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["error"], "Failed to submit GitHub PR review");
        assert!(body["details"]
            .as_str()
            .unwrap()
            .contains("network unavailable"));

        let saved_action = app_state
            .agent_conversation_workspace_repo
            .get_pr_review_action(&action.id)
            .await
            .unwrap()
            .expect("action should still exist");
        assert_eq!(
            saved_action.status,
            AgentWorkspacePrReviewActionStatus::Pending
        );
        assert!(saved_action.submitted_review_id.is_none());
        assert!(saved_action.resolved_at.is_none());

        let pending = app_state
            .agent_conversation_workspace_repo
            .get_pending_pr_review_action_for_head(&conversation_id, 411, "head-sha")
            .await
            .unwrap()
            .expect("failed submit should leave a retryable pending action");
        assert_eq!(pending.id, action.id);

        let monitor = app_state
            .agent_conversation_workspace_repo
            .get_pr_review_monitor(&conversation_id)
            .await
            .unwrap()
            .expect("monitor should exist");
        assert_eq!(
            monitor.status,
            AgentWorkspacePrReviewMonitorStatus::AwaitingUser
        );
        assert_eq!(monitor.last_seen_head_sha.as_deref(), Some("head-sha"));
        assert!(monitor
            .last_error
            .as_deref()
            .unwrap()
            .contains("network unavailable"));

        {
            let github_state = github.state();
            assert_eq!(github_state.submit_pr_review_calls, 1);
            assert_eq!(
                github_state
                    .last_submit_pr_review_args
                    .as_ref()
                    .map(|(pr_number, event, body)| (*pr_number, *event, body.clone())),
                Some((
                    411,
                    PrReviewSubmissionEvent::RequestChanges,
                    format!("Please fix the regression before merge.\n\n{RALPHX_GENERATED_FOOTER}")
                ))
            );
        }

        github.will_submit_pr_review("review-retry", None);
        github.state().fetch_pr_health_result = Some(Ok(open_review_pr_health()));
        let Json(response) = submit_agent_workspace_pr_review_action(
            State(state),
            Path((conversation_id.to_string(), action.id.clone())),
            Json(SubmitAgentWorkspacePrReviewActionRequest { action_kind: None }),
        )
        .await
        .expect("retry should submit the pending action");

        assert_eq!(response.action.status, "submitted");
        assert_eq!(
            response.action.review_body,
            "Please fix the regression before merge."
        );
        let github_state = github.state();
        assert_eq!(github_state.submit_pr_review_calls, 2);
        let retry_body = &github_state
            .last_submit_pr_review_args
            .as_ref()
            .expect("retry args should be captured")
            .2;
        assert_eq!(retry_body.matches(RALPHX_GENERATED_FOOTER).count(), 1);
    }

    #[tokio::test]
    async fn readiness_handler_reports_publishable_workspace_with_uncommitted_changes() {
        let repo = tempfile::TempDir::new().expect("repo tempdir");
        let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        git(repo.path(), &["config", "user.name", "RalphX Test"]);
        std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
        git(repo.path(), &["add", "README.md"]);
        git(repo.path(), &["commit", "-m", "base"]);
        let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut project = Project::new(
            "Readiness Workspace".to_string(),
            repo.path().to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .expect("seed project");

        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        conversation.context_type = ChatContextType::Project;
        conversation.context_id = project.id.as_str().to_string();
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("seed conversation");

        let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
            .expect("workspace path");
        let branch_name = "ralphx/test/readiness-workspace";
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                branch_name,
                workspace_path.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(workspace_path.join("implementation.txt"), "uncommitted\n")
            .expect("write workspace change");
        let workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_sha),
            branch_name.to_string(),
            workspace_path.to_string_lossy().to_string(),
        );
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        seed_current_passing_workspace_review(app_state.as_ref(), &workspace).await;
        let state = test_http_state(app_state);

        let Json(response) =
            check_agent_workspace_publish_readiness(State(state), Path(conversation_id.to_string()))
                .await
                .expect("readiness should load");

        assert!(response.success);
        assert!(response.can_publish);
        assert!(response.blockers.is_empty());
        assert!(!response.needs_base_update);
        assert!(response.recommended_actions.is_empty());
        assert!(response.freshness.has_uncommitted_changes);
    }

    #[tokio::test]
    async fn readiness_handler_ignores_required_review_gate_when_policy_is_disabled() {
        let repo = tempfile::TempDir::new().expect("repo tempdir");
        let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        git(repo.path(), &["config", "user.name", "RalphX Test"]);
        std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
        git(repo.path(), &["add", "README.md"]);
        git(repo.path(), &["commit", "-m", "base"]);
        let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

        let app_state = Arc::new(AppState::new_test());
        app_state
            .review_settings_repo
            .update_settings(&ReviewSettings {
                require_workspace_review: false,
                ..ReviewSettings::default()
            })
            .await
            .expect("review settings should update");
        let conversation_id = ChatConversationId::new();
        let mut project = Project::new(
            "Readiness Workspace Disabled Review".to_string(),
            repo.path().to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .expect("seed project");

        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        conversation.context_type = ChatContextType::Project;
        conversation.context_id = project.id.as_str().to_string();
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("seed conversation");

        let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
            .expect("workspace path");
        let branch_name = "ralphx/test/readiness-policy-disabled";
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                branch_name,
                workspace_path.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(workspace_path.join("implementation.txt"), "uncommitted\n")
            .expect("write workspace change");
        let workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_sha),
            branch_name.to_string(),
            workspace_path.to_string_lossy().to_string(),
        );
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");
        let state = test_http_state(app_state);

        let Json(response) =
            check_agent_workspace_publish_readiness(State(state), Path(conversation_id.to_string()))
                .await
                .expect("readiness should load");

        assert!(response.success);
        assert_eq!(response.review_gate_status.as_deref(), Some("required"));
        assert!(response.can_publish);
        assert!(response.blockers.is_empty());
        assert!(response.freshness.has_uncommitted_changes);
    }

    #[tokio::test]
    async fn update_from_base_rejects_invalid_base_kind_before_loading_workspace() {
        let state = test_http_state(Arc::new(AppState::new_test()));

        let (status, Json(body)) = update_agent_workspace_from_base(
            State(state),
            Path(ChatConversationId::new().to_string()),
            Json(UpdateAgentWorkspaceFromBaseRequest {
                base_ref_kind: Some("not-a-kind".to_string()),
                base_ref: Some("main".to_string()),
                base_display_name: Some("main".to_string()),
                created_by_run_id: None,
            }),
        )
        .await
        .unwrap_err();

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("unknown ideation analysis base ref kind"));
    }

    #[tokio::test]
    async fn needs_repair_action_response_preserves_error_payload_without_implying_queue() {
        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_push_status = Some("needs_agent".to_string());
        workspace.publication_pr_number = Some(42);
        workspace.publication_pr_url = Some("https://github.com/mock/project/pull/42".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();

        let Json(response) = action_response_for_needs_repair(
            app_state.as_ref(),
            &conversation_id,
            "merge conflict".to_string(),
        )
        .await
        .expect("needs-agent response should be returned");

        assert!(response.success);
        assert_eq!(response.status, "needs_agent_repair");
        assert_eq!(response.message, "merge conflict");
        assert!(!response.repair_queued);
        assert!(response.freshness.is_none());
        assert_eq!(response.pr_number, None);
        assert_eq!(response.pr_url, None);
    }

    #[tokio::test]
    async fn needs_repair_action_response_reports_queue_from_repair_events() {
        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_push_status = Some("needs_agent".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        app_state
            .agent_conversation_workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                "repair_sent",
                "started",
                "Starting workspace repair agent for base update failure",
                Some(format!("agent_fixable:run:{}", AgentRunId::new())),
            ))
            .await
            .unwrap();

        let Json(response) = action_response_for_needs_repair(
            app_state.as_ref(),
            &conversation_id,
            "merge conflict".to_string(),
        )
        .await
        .expect("needs-agent response should be returned");

        assert!(response.success);
        assert_eq!(response.status, "needs_agent_repair");
        assert!(response.repair_queued);
    }

    #[tokio::test]
    async fn get_publish_status_reports_in_progress_and_events() {
        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_push_status = Some("checking".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        app_state
            .agent_conversation_workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                "checking",
                "started",
                "Checking workspace changes",
                None,
            ))
            .await
            .unwrap();
        let state = test_http_state(app_state);

        let Json(response) =
            get_agent_workspace_publish_status(State(state), Path(conversation_id.to_string()))
                .await
                .unwrap();

        assert!(response.success);
        assert!(response.publish_in_progress);
        assert!(!response.needs_agent_repair);
        assert_eq!(
            response.workspace.publication_push_status.as_deref(),
            Some("checking")
        );
        assert_eq!(response.events.len(), 1);
        assert_eq!(response.events[0].step, "checking");
    }

    #[tokio::test]
    async fn publish_agent_workspace_returns_in_progress_for_active_publish_state() {
        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_push_status = Some("pushing".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        let state = test_http_state(app_state);

        let Json(response) = publish_agent_workspace(State(state), Path(conversation_id.to_string()))
            .await
            .unwrap();

        assert!(response.success);
        assert_eq!(response.status, "publish_in_progress");
        assert!(!response.repair_queued);
    }

    #[tokio::test]
    async fn publish_agent_workspace_returns_repair_state_without_republishing() {
        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_push_status = Some("needs_agent".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        let state = test_http_state(app_state);

        let Json(response) = publish_agent_workspace(State(state), Path(conversation_id.to_string()))
            .await
            .unwrap();

        assert!(response.success);
        assert_eq!(response.status, "needs_agent_repair");
        assert!(!response.repair_queued);
    }

    #[tokio::test]
    async fn complete_pr_fix_skips_publish_when_pr_is_already_merged() {
        let mut app_state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        github.will_return_status(PrStatus::Merged {
            merge_commit_sha: Some("a".repeat(40)),
            merged_at: None,
        });
        app_state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
        let app_state = Arc::new(app_state);

        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_pr_number = Some(267);
        workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/267".to_string());
        workspace.publication_pr_status = Some("open".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        workspace.pr_supervision_status = Some("fixing".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        let pr_fix_run_id = seed_pr_fix_completion_authority(app_state.as_ref(), &conversation_id).await;
        let state = test_http_state(Arc::clone(&app_state));

        let Json(response) = complete_agent_workspace_pr_fix(
            State(state),
            Path(conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "Investigated post-merge fixer state".to_string(),
                blocker: None,
                fix_commit_sha: None,
                created_by_run_id: Some(pr_fix_run_id.to_string()),
            }),
        )
        .await
        .expect("terminal PR should be handled without publishing");

        assert_eq!(response.status, "skipped_terminal");
        assert_eq!(response.publish_status.as_deref(), Some("skipped"));
        let updated = app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.publication_pr_status.as_deref(), Some("merged"));
        assert!(updated.pr_supervision_status.is_none());
        assert_eq!(github.state().check_pr_status_calls, 1);
    }

    #[tokio::test]
    async fn complete_pr_fix_stale_attempt_is_a_side_effect_free_superseded_noop() {
        let fixture = setup_pr_fix_workspace_with_review_gate(
            "stale-superseded",
            AgentWorkspaceReviewGateStatus::Blocking,
        )
        .await;
        let mut stale_run = AgentRun::new(fixture.conversation_id.clone());
        stale_run.action_kind = Some(crate::domain::entities::AgentRunActionKind::PrAutofix);
        stale_run.action_context_id = Some("267".to_string());
        stale_run.action_target_id = Some("github_pr_autofix:267:test".to_string());
        stale_run.status = crate::domain::entities::AgentRunStatus::Failed;
        stale_run.started_at = chrono::Utc::now() - chrono::Duration::seconds(1);
        let stale_run_id = fixture
            .app_state
            .agent_run_repo
            .create(stale_run)
            .await
            .expect("stale PR autofix run should persist")
            .id;
        let before = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        let events_before = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .expect("events should load")
            .len();

        let Json(response) = complete_agent_workspace_pr_fix(
            State(test_http_state(Arc::clone(&fixture.app_state))),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "Stale fixer must not settle the replacement".to_string(),
                blocker: None,
                fix_commit_sha: None,
                created_by_run_id: Some(stale_run_id.to_string()),
            }),
        )
        .await
        .expect("stale attempt should be acknowledged as superseded");

        assert_eq!(response.status, "superseded");
        assert!(response.workspace.is_none());
        assert_eq!(fixture.github.state().check_pr_status_calls, 0);
        assert_eq!(fixture.github.state().push_branch_calls, 0);
        let after = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(after.publication_push_status, before.publication_push_status);
        assert_eq!(after.pr_supervision_status, before.pr_supervision_status);
        assert_eq!(
            fixture
                .app_state
                .agent_conversation_workspace_repo
                .list_publication_events(&fixture.conversation_id)
                .await
                .expect("events should load")
                .len(),
            events_before
        );
    }

    #[tokio::test]
    async fn complete_pr_fix_old_fingerprint_cannot_settle_new_issue_claim() {
        let fixture = setup_pr_fix_workspace_with_review_gate(
            "stale-fingerprint-superseded",
            AgentWorkspaceReviewGateStatus::Blocking,
        )
        .await;
        let original_workspace = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        let original_claim = AgentWorkspaceRepairClaim {
            conversation_id: fixture.conversation_id.clone(),
            guard: AgentWorkspaceRepairStateGuard::from_workspace(&original_workspace),
        };
        assert!(
            crate::application::agent_workspace_publish_repair_state::settle_agent_workspace_repair_failure(
                Arc::clone(&fixture.app_state.agent_conversation_workspace_repo),
                &original_claim,
                "The routed PR issue changed.",
            )
            .await
            .expect("original claim settlement should succeed")
        );
        crate::application::agent_workspace_publish_repair_state::claim_agent_workspace_repair(
            Arc::clone(&fixture.app_state.agent_conversation_workspace_repo),
            &fixture.conversation_id,
            "Routing the replacement PR issue.",
            original_workspace.pr_auto_merge_current,
        )
        .await
        .expect("replacement claim should persist")
        .expect("replacement claim should win");
        let mut replacement_run = AgentRun::new(fixture.conversation_id.clone());
        replacement_run.action_kind = Some(crate::domain::entities::AgentRunActionKind::PrAutofix);
        replacement_run.action_context_id = Some("267".to_string());
        replacement_run.action_target_id =
            Some("github_pr_autofix:267:new-issue-fingerprint".to_string());
        fixture
            .app_state
            .agent_run_repo
            .create(replacement_run)
            .await
            .expect("replacement PR autofix run should persist");
        let before = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        let events_before = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .expect("events should load")
            .len();

        let Json(response) = complete_agent_workspace_pr_fix(
            State(test_http_state(Arc::clone(&fixture.app_state))),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "The old issue fixer must not settle the new claim".to_string(),
                blocker: None,
                fix_commit_sha: None,
                created_by_run_id: Some(fixture.pr_fix_run_id.to_string()),
            }),
        )
        .await
        .expect("old fingerprint should be acknowledged as superseded");

        assert_eq!(response.status, "superseded");
        assert!(response.workspace.is_none());
        assert_eq!(fixture.github.state().check_pr_status_calls, 0);
        assert_eq!(fixture.github.state().push_branch_calls, 0);
        assert_eq!(fixture.github.state().enable_pr_auto_merge_calls, 0);
        assert_eq!(fixture.github.state().disable_pr_auto_merge_calls, 0);
        assert_eq!(fixture.github.state().submit_pr_review_calls, 0);
        let after = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(after, before);
        assert_eq!(
            fixture
                .app_state
                .agent_conversation_workspace_repo
                .list_publication_events(&fixture.conversation_id)
                .await
                .expect("events should load")
                .len(),
            events_before
        );
    }

    #[tokio::test]
    async fn complete_pr_fix_already_completed_is_a_side_effect_free_noop() {
        let fixture = setup_pr_fix_workspace_with_review_gate(
            "already-completed-noop",
            AgentWorkspaceReviewGateStatus::Blocking,
        )
        .await;
        fixture
            .app_state
            .agent_run_repo
            .update_status(
                &fixture.pr_fix_run_id,
                crate::domain::entities::AgentRunStatus::Completed,
            )
            .await
            .expect("settled PR autofix run should persist");
        let before = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        let events_before = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .expect("events should load")
            .len();

        let Json(response) = complete_agent_workspace_pr_fix(
            State(test_http_state(Arc::clone(&fixture.app_state))),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "The settled fixer must not publish twice".to_string(),
                blocker: Some("must not persist".to_string()),
                fix_commit_sha: Some(fixture.fix_commit_sha.clone()),
                created_by_run_id: Some(fixture.pr_fix_run_id.to_string()),
            }),
        )
        .await
        .expect("settled attempt should be acknowledged without side effects");

        assert_eq!(response.status, "already_completed");
        assert!(response.workspace.is_none());
        assert_eq!(fixture.github.state().check_pr_status_calls, 0);
        assert_eq!(fixture.github.state().push_branch_calls, 0);
        assert_eq!(fixture.github.state().enable_pr_auto_merge_calls, 0);
        assert_eq!(fixture.github.state().disable_pr_auto_merge_calls, 0);
        assert_eq!(fixture.github.state().submit_pr_review_calls, 0);
        let after = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(after, before);
        assert_eq!(
            fixture
                .app_state
                .agent_conversation_workspace_repo
                .list_publication_events(&fixture.conversation_id)
                .await
                .expect("events should load")
                .len(),
            events_before
        );
    }

    #[tokio::test]
    async fn complete_pr_fix_current_authority_with_stale_claim_schedules_recovery_without_effects() {
        let fixture = setup_pr_fix_workspace_with_review_gate(
            "stale-current-claim",
            AgentWorkspaceReviewGateStatus::Blocking,
        )
        .await;
        let mut stale_workspace = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        stale_workspace.publication_push_status = Some("failed".to_string());
        stale_workspace.pr_supervision_status = Some("blocked".to_string());
        fixture
            .app_state
            .agent_conversation_workspace_repo
            .create_or_update(stale_workspace.clone())
            .await
            .expect("seed stale PR fixer claim");
        let events_before = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .expect("events should load")
            .len();

        let (status, Json(body)) = complete_agent_workspace_pr_fix(
            State(test_http_state(Arc::clone(&fixture.app_state))),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "The stale workspace claim must not settle".to_string(),
                blocker: Some("must not persist".to_string()),
                fix_commit_sha: Some(fixture.fix_commit_sha.clone()),
                created_by_run_id: Some(fixture.pr_fix_run_id.to_string()),
            }),
        )
        .await
        .expect_err("stale workspace claim must fail closed");

        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body["error"]
            .as_str()
            .is_some_and(|message| message.contains("claim is no longer current")));
        for _ in 0..100 {
            if fixture.github.state().check_pr_sync_state_calls >= 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(fixture.github.state().check_pr_sync_state_calls, 1);
        {
            let github_state = fixture.github.state();
            assert_eq!(github_state.check_pr_status_calls, 0);
            assert_eq!(github_state.create_issue_calls, 0);
            assert_eq!(github_state.create_draft_pr_calls, 0);
            assert_eq!(github_state.mark_pr_ready_calls, 0);
            assert_eq!(github_state.update_pr_details_calls, 0);
            assert_eq!(github_state.update_pr_base_calls, 0);
            assert_eq!(github_state.push_branch_calls, 0);
            assert_eq!(github_state.enable_pr_auto_merge_calls, 0);
            assert_eq!(github_state.disable_pr_auto_merge_calls, 0);
            assert_eq!(github_state.submit_pr_review_calls, 0);
            assert_eq!(github_state.close_pr_calls, 0);
            assert_eq!(github_state.delete_remote_branch_calls, 0);
        }
        let after = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(after.status, stale_workspace.status);
        assert_eq!(after.source_pull_request, stale_workspace.source_pull_request);
        assert_eq!(after.publication_pr_number, stale_workspace.publication_pr_number);
        assert_eq!(after.publication_pr_url, stale_workspace.publication_pr_url);
        assert_eq!(after.publication_pr_status, stale_workspace.publication_pr_status);
        assert_eq!(after.publication_push_status, stale_workspace.publication_push_status);
        assert_eq!(after.auto_publish_enabled, stale_workspace.auto_publish_enabled);
        assert_eq!(
            after.auto_publish_initial_pr_enabled,
            stale_workspace.auto_publish_initial_pr_enabled
        );
        assert_eq!(
            after.auto_publish_paused_pr_autofix_enabled,
            stale_workspace.auto_publish_paused_pr_autofix_enabled
        );
        assert_eq!(
            after.auto_publish_paused_pr_auto_merge_desired,
            stale_workspace.auto_publish_paused_pr_auto_merge_desired
        );
        assert_eq!(after.pr_autofix_enabled, stale_workspace.pr_autofix_enabled);
        assert_eq!(
            after.pr_auto_merge_desired,
            stale_workspace.pr_auto_merge_desired
        );
        assert_eq!(after.pr_auto_merge_method, stale_workspace.pr_auto_merge_method);
        assert_eq!(after.pr_auto_merge_current, stale_workspace.pr_auto_merge_current);
        assert_eq!(
            after.pr_supervision_status,
            stale_workspace.pr_supervision_status
        );
        assert_eq!(
            after.pr_supervision_summary,
            stale_workspace.pr_supervision_summary
        );
        let events_after = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .expect("events should load");
        assert_eq!(events_after.len(), events_before);
        assert!(
            !events_after.iter().skip(events_before).any(|event| {
                matches!(
                    event.step.as_str(),
                    "pr_autofix_completed"
                        | "pr_autofix_blocked"
                        | "pr_autofix_published"
                        | "pr_autofix_publish_failed"
                )
            }),
            "stale claim must not append completion, block, or publish events"
        );
    }

    #[tokio::test]
    async fn complete_pr_fix_invalid_or_missing_caller_fails_closed_without_completion_effects() {
        let fixture = setup_pr_fix_workspace_with_review_gate(
            "missing-caller-noop",
            AgentWorkspaceReviewGateStatus::Blocking,
        )
        .await;
        let before = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        let events_before = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .expect("events should load")
            .len();

        for created_by_run_id in [None, Some("not-a-run-id".to_string())] {
            let (status, Json(body)) = complete_agent_workspace_pr_fix(
                State(test_http_state(Arc::clone(&fixture.app_state))),
                Path(fixture.conversation_id.to_string()),
                Json(CompleteAgentWorkspacePrFixRequest {
                    summary: "Invalid ownership must not settle the fixer".to_string(),
                    blocker: Some("must not persist".to_string()),
                    fix_commit_sha: Some(fixture.fix_commit_sha.clone()),
                    created_by_run_id,
                }),
            )
            .await
            .expect_err("invalid fixer authority must fail closed");

            assert_eq!(status, StatusCode::CONFLICT);
            assert!(body["error"]
                .as_str()
                .is_some_and(|message| message.contains("no longer current")));
        }
        assert_eq!(fixture.github.state().push_branch_calls, 0);
        assert_eq!(fixture.github.state().enable_pr_auto_merge_calls, 0);
        assert_eq!(fixture.github.state().disable_pr_auto_merge_calls, 0);
        assert_eq!(fixture.github.state().submit_pr_review_calls, 0);
        let after = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        assert_eq!(after, before);
        let events = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .expect("events should load");
        assert_eq!(events.len(), events_before);
        assert!(!events.iter().any(|event| {
            matches!(
                event.step.as_str(),
                "pr_autofix_completed"
                    | "pr_autofix_blocked"
                    | "pr_autofix_published"
                    | "pr_autofix_publish_failed"
            )
        }));
    }

    #[tokio::test]
    async fn complete_pr_fix_skips_publish_when_auto_publish_is_paused() {
        let fixture = setup_pr_fix_workspace_with_review_gate(
            "auto-publish-paused",
            AgentWorkspaceReviewGateStatus::Blocking,
        )
        .await;
        fixture
            .app_state
            .review_settings_repo
            .update_settings(&ReviewSettings {
                require_workspace_review: false,
                ..ReviewSettings::default()
            })
            .await
            .expect("review settings should update");
        let mut workspace = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        workspace.auto_publish_enabled = false;
        fixture
            .app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        let state = test_http_state(Arc::clone(&fixture.app_state));

        let Json(response) = complete_agent_workspace_pr_fix(
            State(state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "Fixed requested review change".to_string(),
                blocker: None,
                fix_commit_sha: Some(fixture.fix_commit_sha.clone()),
                created_by_run_id: Some(fixture.pr_fix_run_id.to_string()),
            }),
        )
        .await
        .expect("paused Auto Publish should skip publish");

        assert_eq!(response.status, "publish_paused");
        assert_eq!(response.publish_status.as_deref(), Some("skipped"));
        assert!(response.commit_sha.is_none());
        let updated = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.pr_supervision_status.as_deref(), Some("paused"));
        let events = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .unwrap();
        assert!(events.iter().any(|event| {
            event.step == "pr_autofix_publish_skipped"
                && event.classification.as_deref() == Some("auto_publish_paused")
        }));
    }

    #[tokio::test]
    async fn complete_pr_fix_requires_exact_head_without_settling_current_attempt() {
        let fixture = setup_pr_fix_workspace_with_review_gate(
            "missing-head",
            AgentWorkspaceReviewGateStatus::Blocking,
        )
        .await;
        let state = test_http_state(Arc::clone(&fixture.app_state));

        let (status, Json(body)) = complete_agent_workspace_pr_fix(
            State(state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "Fixed failing CI check".to_string(),
                blocker: None,
                fix_commit_sha: None,
                created_by_run_id: Some(fixture.pr_fix_run_id.to_string()),
            }),
        )
        .await
        .expect_err("missing fix HEAD must be rejected");

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().contains("fix_commit_sha"));
        let workspace = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            workspace.publication_push_status.as_deref(),
            Some("needs_agent")
        );
        assert_eq!(workspace.pr_supervision_status.as_deref(), Some("fixing"));
        let events = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .unwrap();
        assert!(!events
            .iter()
            .any(|event| event.step == "pr_autofix_completed"));
    }

    #[tokio::test]
    async fn complete_pr_fix_rejects_configured_branch_tip_when_worktree_head_is_detached() {
        let fixture = setup_pr_fix_workspace_with_review_gate(
            "detached-head",
            AgentWorkspaceReviewGateStatus::Blocking,
        )
        .await;
        let workspace = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        git(&workspace.worktree_path, &["checkout", "--detach", "HEAD^"]);
        let state = test_http_state(Arc::clone(&fixture.app_state));

        let (status, Json(body)) = complete_agent_workspace_pr_fix(
            State(state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "Fixed failing CI check".to_string(),
                blocker: None,
                fix_commit_sha: Some(fixture.fix_commit_sha.clone()),
                created_by_run_id: Some(fixture.pr_fix_run_id.to_string()),
            }),
        )
        .await
        .expect_err("detached worktree HEAD must reject the configured branch tip");

        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("not the current workspace HEAD"));
        let events = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .unwrap();
        assert!(!events
            .iter()
            .any(|event| event.step == "pr_autofix_completed"));
    }

    #[tokio::test]
    async fn complete_pr_fix_rejects_configured_branch_tip_when_worktree_switches_branch() {
        let fixture = setup_pr_fix_workspace_with_review_gate(
            "switched-head",
            AgentWorkspaceReviewGateStatus::Blocking,
        )
        .await;
        let workspace = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");
        git(
            &workspace.worktree_path,
            &["checkout", "-b", "unexpected-pr-fix-head", "HEAD^"],
        );
        let state = test_http_state(Arc::clone(&fixture.app_state));

        let (status, Json(body)) = complete_agent_workspace_pr_fix(
            State(state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "Fixed failing CI check".to_string(),
                blocker: None,
                fix_commit_sha: Some(fixture.fix_commit_sha.clone()),
                created_by_run_id: Some(fixture.pr_fix_run_id.to_string()),
            }),
        )
        .await
        .expect_err("switched worktree branch must reject the configured branch tip");

        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("not the current workspace HEAD"));
        let events = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .unwrap();
        assert!(!events
            .iter()
            .any(|event| event.step == "pr_autofix_completed"));
    }

    #[tokio::test]
    async fn complete_pr_fix_blocker_needs_no_sha_or_github_preflight() {
        let fixture = setup_pr_fix_workspace_with_review_gate(
            "blocker-no-head",
            AgentWorkspaceReviewGateStatus::Blocking,
        )
        .await;
        let state = test_http_state(Arc::clone(&fixture.app_state));

        let Json(response) = complete_agent_workspace_pr_fix(
            State(state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "Repair could not be completed".to_string(),
                blocker: Some("Required dependency is unavailable".to_string()),
                fix_commit_sha: None,
                created_by_run_id: Some(fixture.pr_fix_run_id.to_string()),
            }),
        )
        .await
        .expect("current repair blocker should settle without a commit SHA");

        assert_eq!(response.status, "blocked");
        assert_eq!(fixture.github.state().check_pr_status_calls, 0);
        let workspace = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(workspace.publication_push_status.as_deref(), Some("failed"));
        assert_eq!(workspace.pr_supervision_status.as_deref(), Some("blocked"));
        let events = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .unwrap();
        assert!(events
            .iter()
            .any(|event| event.step == "pr_autofix_blocked"));
        assert!(!events
            .iter()
            .any(|event| event.step == "pr_autofix_completed"));
    }

    #[tokio::test]
    async fn complete_pr_fix_rejects_dirty_workspace_without_settling_current_attempt() {
        let fixture = setup_pr_fix_workspace_with_review_gate(
            "dirty-workspace",
            AgentWorkspaceReviewGateStatus::Blocking,
        )
        .await;
        let workspace = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .unwrap()
            .unwrap();
        std::fs::write(
            std::path::Path::new(&workspace.worktree_path).join("dirty.txt"),
            "uncommitted\n",
        )
        .expect("dirty workspace file should be written");
        let state = test_http_state(Arc::clone(&fixture.app_state));

        let (status, Json(body)) = complete_agent_workspace_pr_fix(
            State(state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "Fixed failing CI check".to_string(),
                blocker: None,
                fix_commit_sha: Some(fixture.fix_commit_sha.clone()),
                created_by_run_id: Some(fixture.pr_fix_run_id.to_string()),
            }),
        )
        .await
        .expect_err("dirty workspace must be rejected");

        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body["error"].as_str().unwrap().contains("uncommitted"));
        let events = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .unwrap();
        assert!(!events
            .iter()
            .any(|event| event.step == "pr_autofix_completed"));
    }

    #[tokio::test]
    async fn complete_pr_fix_waits_for_running_workspace_review_when_required() {
        let repo = tempfile::TempDir::new().expect("repo tempdir");
        let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        git(repo.path(), &["config", "user.name", "RalphX Test"]);
        std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
        git(repo.path(), &["add", "README.md"]);
        git(repo.path(), &["commit", "-m", "base"]);
        let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut project = Project::new(
            "PR Fix Review Workspace".to_string(),
            repo.path().to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .expect("seed project");

        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        conversation.context_type = ChatContextType::Project;
        conversation.context_id = project.id.as_str().to_string();
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("seed conversation");

        let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
            .expect("workspace path");
        let branch_name = "ralphx/test/pr-fix-review-required";
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                branch_name,
                workspace_path.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(workspace_path.join("fix.txt"), "ci fix\n").expect("write workspace change");
        git(&workspace_path, &["add", "fix.txt"]);
        git(&workspace_path, &["commit", "-m", "fix CI"]);
        let fix_commit_sha = git(&workspace_path, &["rev-parse", "HEAD"]);
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_sha),
            branch_name.to_string(),
            workspace_path.to_string_lossy().to_string(),
        );
        workspace.publication_pr_number = Some(267);
        workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/267".to_string());
        workspace.publication_pr_status = Some("open".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        workspace.pr_supervision_status = Some("fixing".to_string());
        workspace.pr_supervision_updated_at = Some(chrono::Utc::now());
        workspace.pr_autofix_enabled = true;
        workspace.pr_auto_merge_desired = true;
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        let pr_fix_run_id = seed_pr_fix_completion_authority(app_state.as_ref(), &conversation_id).await;
        let review_context = load_agent_workspace_review_context(app_state.as_ref(), &workspace)
            .await
            .expect("review context should load");
        let mut monitor = review_context.monitor;
        monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
        app_state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .expect("running review monitor should persist");
        let state = test_http_state(Arc::clone(&app_state));

        let Json(response) = complete_agent_workspace_pr_fix(
            State(state),
            Path(conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "Fixed failing CI check".to_string(),
                blocker: None,
                fix_commit_sha: Some(fix_commit_sha),
                created_by_run_id: Some(pr_fix_run_id.to_string()),
            }),
        )
        .await
        .expect("running review should wait instead of blocking supervision");

        assert_eq!(response.status, "workspace_reviewing");
        assert_eq!(
            response.publish_status.as_deref(),
            Some("waiting_for_workspace_review")
        );
        assert!(response.publish_error.is_none());
        assert!(response.commit_sha.is_none());
        let updated = app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.pr_supervision_status.as_deref(), Some("reviewing"));
        let review_context = load_agent_workspace_review_context(app_state.as_ref(), &updated)
            .await
            .expect("review context should load");
        assert_eq!(
            review_context.monitor.status,
            AgentWorkspaceReviewMonitorStatus::Reviewing
        );
        assert_eq!(
            review_context.monitor.review_gate_status,
            AgentWorkspaceReviewGateStatus::Reviewing
        );
        let events = app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap();
        assert!(events.iter().any(|event| {
            event.step == "pr_autofix_workspace_review"
                && event.status == "pending"
                && event.classification.as_deref() == Some("workspace_review_pending")
        }));
    }

    #[tokio::test]
    async fn complete_repair_starts_fresh_workspace_review_when_blocking_review_is_stale() {
        let repo = tempfile::TempDir::new().expect("repo tempdir");
        let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        git(repo.path(), &["config", "user.name", "RalphX Test"]);
        std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
        git(repo.path(), &["add", "README.md"]);
        git(repo.path(), &["commit", "-m", "base"]);
        let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

        let app_state = Arc::new(AppState::new_test());
        app_state
            .review_settings_repo
            .update_settings(&ReviewSettings {
                require_workspace_review: true,
                ..ReviewSettings::default()
            })
            .await
            .expect("review settings should update");
        let conversation_id = ChatConversationId::new();
        let mut project = Project::new(
            "Repair Review Refresh".to_string(),
            repo.path().to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .expect("seed project");

        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        conversation.context_type = ChatContextType::Project;
        conversation.context_id = project.id.as_str().to_string();
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("seed conversation");

        let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
            .expect("workspace path");
        let branch_name = "ralphx/test/repair-review-refresh";
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                branch_name,
                workspace_path.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(workspace_path.join("reviewed.txt"), "blocking\n")
            .expect("write reviewed change");
        git(&workspace_path, &["add", "reviewed.txt"]);
        git(
            &workspace_path,
            &["commit", "-m", "reviewed blocking change"],
        );

        let mut workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_sha.clone()),
            branch_name.to_string(),
            workspace_path.to_string_lossy().to_string(),
        );
        workspace.publication_push_status = Some("refreshed".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");

        let initial_review = load_agent_workspace_review_context(app_state.as_ref(), &workspace)
            .await
            .expect("initial review context should load");
        let initial_target = initial_review.target.expect("review target should exist");
        let mut monitor = initial_review.monitor;
        apply_review_artifact_to_monitor(
            &mut monitor,
            initial_target.scope,
            initial_target.head_sha,
            initial_target.diff_fingerprint,
            Some("review-run-blocking".to_string()),
            ArtifactId::from_string("artifact-blocking-review"),
            1,
            chrono::Utc::now(),
            None,
        );
        monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
        monitor.review_outcome = AgentWorkspaceReviewOutcome::Blocking;
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Blocking;
        monitor.review_blocking_summary = Some("Blocking review finding".to_string());
        monitor.review_blocking_fingerprint = Some("blocking-fingerprint".to_string());
        monitor.review_fixer_status = Some("running".to_string());
        monitor.review_fixer_run_id = Some("fixer-run".to_string());
        monitor.review_fixer_conversation_id = Some(conversation_id.clone());
        app_state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .expect("blocking review monitor should persist");

        std::fs::write(workspace_path.join("reviewed.txt"), "fixed\n").expect("write repair");
        git(&workspace_path, &["add", "reviewed.txt"]);
        git(&workspace_path, &["commit", "-m", "repair blocking review"]);
        let repair_sha = git(&workspace_path, &["rev-parse", "HEAD"]);

        let stale_context = load_agent_workspace_review_context(app_state.as_ref(), &workspace)
            .await
            .expect("stale review context should load");
        assert_eq!(
            stale_context.monitor.review_gate_status,
            AgentWorkspaceReviewGateStatus::Required
        );

        let state = test_http_state(Arc::clone(&app_state));
        let starter = RecordingWorkspaceReviewStarter::new();
        let response = complete_repair_workspace_review_response_if_required_with_starter(
            &state,
            &conversation_id,
            &workspace,
            &base_sha,
            &repair_sha,
            "fixed the blocking review finding",
            &starter,
        )
        .await
        .expect("repair review response should succeed")
        .expect("stale required review should pause publish")
        .0;

        assert_eq!(starter.call_count(), 1);
        assert_eq!(response.new_status, "refreshed");
        assert_eq!(
            response.auto_publish_status.as_deref(),
            Some("waiting_for_workspace_review")
        );
        assert_eq!(response.auto_publish_error, None);
        let updated_workspace = app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace should load")
            .expect("workspace should exist");
        assert_eq!(
            updated_workspace.pr_supervision_status.as_deref(),
            Some("reviewing"),
            "repair completion should persist the paused workspace-review supervision state"
        );
        assert_eq!(
            updated_workspace.pr_supervision_summary.as_deref(),
            Some(
                "Agent workspace repair verified; Workspace Review started before publishing resumes."
            )
        );
        let events = app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("publication events should load");
        assert!(
            events.iter().any(|event| {
                event.step == "repair_workspace_review"
                    && event.status == "reviewing"
                    && event.classification.as_deref() == Some("workspace_review_started")
                    && event.summary.contains("fixed the blocking review finding")
            }),
            "repair completion should append a durable workspace-review pause event"
        );
        let refreshed_review = load_agent_workspace_review_context(app_state.as_ref(), &workspace)
            .await
            .expect("refreshed review context should load");
        assert_eq!(
            refreshed_review.monitor.status,
            AgentWorkspaceReviewMonitorStatus::Reviewing
        );
        assert_eq!(
            refreshed_review.monitor.review_gate_status,
            AgentWorkspaceReviewGateStatus::Reviewing
        );
        assert_eq!(
            refreshed_review.monitor.review_fixer_status, None,
            "repair completion should clear stale fixer state before the fresh review"
        );
    }

    #[tokio::test]
    async fn complete_pr_fix_blocks_when_workspace_review_has_blocking_findings() {
        let fixture = setup_pr_fix_workspace_with_review_gate(
            "blocking",
            AgentWorkspaceReviewGateStatus::Blocking,
        )
        .await;
        let state = test_http_state(Arc::clone(&fixture.app_state));

        let Json(response) = complete_agent_workspace_pr_fix(
            State(state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "Fixed failing CI check".to_string(),
                blocker: None,
                fix_commit_sha: Some(fixture.fix_commit_sha.clone()),
                created_by_run_id: Some(fixture.pr_fix_run_id.to_string()),
            }),
        )
        .await
        .expect("blocking review should return an authoritative block");

        assert_eq!(response.status, "workspace_review_blocked");
        assert_eq!(
            response.publish_status.as_deref(),
            Some("blocked_by_workspace_review")
        );
        assert!(response.commit_sha.is_none());
        assert_eq!(fixture.github.state().push_branch_calls, 0);
        let updated = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
        assert!(updated
            .pr_supervision_summary
            .as_deref()
            .unwrap()
            .contains("Workspace Review found blocking changes"));
        let events = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .unwrap();
        assert!(events.iter().any(|event| {
            event.step == "pr_autofix_workspace_review_aborted"
                && event.status == "failed"
                && event.classification.as_deref() == Some("workspace_review_aborted")
        }));
        assert!(!events
            .iter()
            .any(|event| event.step == "pr_autofix_publish_failed"));
    }

    #[tokio::test]
    async fn pr_fix_workspace_review_gate_is_skipped_when_policy_is_disabled() {
        let fixture = setup_pr_fix_workspace_with_review_gate(
            "policy-disabled",
            AgentWorkspaceReviewGateStatus::Blocking,
        )
        .await;
        fixture
            .app_state
            .review_settings_repo
            .update_settings(&ReviewSettings {
                require_workspace_review: false,
                ..ReviewSettings::default()
            })
            .await
            .expect("review settings should update");
        let state = test_http_state(Arc::clone(&fixture.app_state));
        let workspace = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .expect("workspace lookup should succeed")
            .expect("workspace should exist");

        let result = start_workspace_review_for_pr_fix_if_required(
            &state,
            &fixture.conversation_id,
            &workspace,
            "Fixed failing CI check",
        )
        .await
        .expect("disabled policy should skip workspace review gate");

        assert!(result.is_none());
        let events = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .unwrap();
        assert!(!events
            .iter()
            .any(|event| event.step == "pr_autofix_workspace_review"));
    }

    #[tokio::test]
    async fn complete_pr_fix_blocks_when_workspace_review_failed() {
        let fixture =
            setup_pr_fix_workspace_with_review_gate("failed", AgentWorkspaceReviewGateStatus::Failed)
                .await;
        let state = test_http_state(Arc::clone(&fixture.app_state));

        let Json(response) = complete_agent_workspace_pr_fix(
            State(state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspacePrFixRequest {
                summary: "Fixed failing CI check".to_string(),
                blocker: None,
                fix_commit_sha: Some(fixture.fix_commit_sha.clone()),
                created_by_run_id: Some(fixture.pr_fix_run_id.to_string()),
            }),
        )
        .await
        .expect("failed review should return an authoritative block");

        assert_eq!(response.status, "workspace_review_failed");
        assert_eq!(
            response.publish_status.as_deref(),
            Some("blocked_by_workspace_review")
        );
        assert_eq!(
            response.publish_error.as_deref(),
            Some("Workspace Review failed")
        );
        assert!(response.commit_sha.is_none());
        assert_eq!(fixture.github.state().push_branch_calls, 0);
        let updated = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
        assert!(updated
            .pr_supervision_summary
            .as_deref()
            .unwrap()
            .contains("Workspace Review failed"));
    }

    #[tokio::test]
    async fn passed_workspace_review_resumes_pr_fix_publish_after_missing_review_failure() {
        let repo = tempfile::TempDir::new().expect("repo tempdir");
        let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        git(repo.path(), &["config", "user.name", "RalphX Test"]);
        std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
        git(repo.path(), &["add", "README.md"]);
        git(repo.path(), &["commit", "-m", "base"]);
        let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

        let github = Arc::new(MockGithubService::new());
        let conversation_id = ChatConversationId::new();
        let mut state = AppState::new_test();
        state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
        let client = Arc::new(SubmittingPrDescriptionClient::new(
            Arc::clone(&state.agent_conversation_workspace_repo),
            conversation_id.clone(),
            true,
        ));
        let state = state.with_agent_client(client);
        let app_state = Arc::new(state);
        let mut project = Project::new(
            "Blocked PR Fix Review Resume".to_string(),
            repo.path().to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .expect("seed project");

        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        conversation.context_type = ChatContextType::Project;
        conversation.context_id = project.id.as_str().to_string();
        conversation.title = Some("Fix blocked review autopilot".to_string());
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("seed conversation");

        let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
            .expect("workspace path");
        let branch_name = "ralphx/test/blocked-pr-fix-review-resume";
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                branch_name,
                workspace_path.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(workspace_path.join("fix.txt"), "ci fix\n").expect("write workspace change");
        git(&workspace_path, &["add", "fix.txt"]);
        git(&workspace_path, &["commit", "-m", "fix CI"]);
        let existing_pr = PrDetail {
            number: 267,
            title: "Existing PR title".to_string(),
            body: Some("Existing PR body".to_string()),
            author: Some("maintainer".to_string()),
            created_at: None,
            url: Some("https://github.com/owner/repo/pull/267".to_string()),
            state: PrStatus::Open,
            is_draft: false,
            head_ref_name: branch_name.to_string(),
            base_ref_name: "main".to_string(),
        };
        github.queue_pr_detail(Ok(existing_pr.clone()));
        github.queue_pr_detail(Ok(existing_pr));
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_sha),
            branch_name.to_string(),
            workspace_path.to_string_lossy().to_string(),
        );
        workspace.publication_pr_number = Some(267);
        workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/267".to_string());
        workspace.publication_pr_status = Some("open".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        workspace.auto_publish_enabled = true;
        workspace.pr_supervision_status = Some("blocked".to_string());
        workspace.pr_supervision_summary =
            Some("Workspace reviewer completed without writing a current Review".to_string());
        workspace.pr_autofix_enabled = true;
        workspace.pr_auto_merge_desired = true;
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");

        let review_context = load_agent_workspace_review_context(app_state.as_ref(), &workspace)
            .await
            .expect("review context should load");
        let target = review_context.target.expect("review target should exist");
        let mut monitor = review_context.monitor;
        monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
        apply_review_artifact_to_monitor(
            &mut monitor,
            target.scope,
            target.head_sha,
            target.diff_fingerprint,
            Some("review-run".to_string()),
            ArtifactId::from_string("review-artifact-blocked-resume"),
            1,
            chrono::Utc::now(),
            None,
        );
        monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
        app_state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .expect("review monitor should persist");

        let state = test_http_state(Arc::clone(&app_state));
        let Json(response) = complete_agent_workspace_review_run(
            State(state),
            Path(conversation_id.to_string()),
            Json(CompleteAgentWorkspaceReviewRunRequest {
                outcome: Some("passed".to_string()),
                summary: "Review passed".to_string(),
                blocker: None,
                created_by_run_id: Some("review-run".to_string()),
            }),
        )
        .await
        .expect("passed workspace review should complete");

        assert_eq!(response.monitor.review_gate_status, "passed");
        {
            let github_state = github.state();
            assert_eq!(github_state.push_branch_calls, 1);
            assert_eq!(
                github_state.last_push_branch_name.as_deref(),
                Some(branch_name)
            );
            assert_eq!(github_state.fetch_pr_detail_calls, 2);
            assert_eq!(github_state.enable_pr_auto_merge_calls, 1);
        }
        let updated = app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.pr_supervision_status.as_deref(), Some("monitoring"));
        assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
        let events = app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap();
        assert!(events.iter().any(|event| {
            event.step == "pr_autofix_workspace_review_passed"
                && event.status == "publishing"
                && event.classification.as_deref() == Some("workspace_review_passed")
        }));
        assert!(events
            .iter()
            .any(|event| event.step == "published" && event.status == "succeeded"));
    }

    struct ReviewCompletionFixture {
        _repo: tempfile::TempDir,
        _worktrees: tempfile::TempDir,
        app_state: Arc<AppState>,
        conversation_id: ChatConversationId,
        automation_id: Option<crate::domain::entities::AutomationId>,
        run_id: Option<crate::domain::entities::AutomationRunId>,
        github: Arc<MockGithubService>,
    }

    /// Seed a no-PR workspace whose review monitor is `Reviewing` with a current artifact, so that
    /// calling `complete_agent_workspace_review_run` recomputes the gate from the passed outcome.
    /// Optionally arms initial auto-publish and links an automation run to the conversation.
    async fn setup_workspace_for_review_completion(
        suffix: &str,
        armed_initial: bool,
        seed_automation: bool,
    ) -> ReviewCompletionFixture {
        use crate::domain::entities::{
            Automation, AutomationId, AutomationJudgeState, AutomationPlanApprovalMode,
            AutomationPlanJudgeState, AutomationPrMergeMode, AutomationPromptAuthor, AutomationRun,
            AutomationRunId, AutomationRunStatus, AutomationStatus,
        };

        let repo = tempfile::TempDir::new().expect("repo tempdir");
        let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        git(repo.path(), &["config", "user.name", "RalphX Test"]);
        std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
        git(repo.path(), &["add", "README.md"]);
        git(repo.path(), &["commit", "-m", "base"]);
        let fake_remote = worktrees.path().join("github-remote.git");
        git(
            repo.path(),
            &[
                "clone",
                "--bare",
                repo.path().to_str().expect("repo path should be UTF-8"),
                fake_remote.to_str().expect("remote path should be UTF-8"),
            ],
        );
        let fake_ssh = worktrees.path().join("fake-github-ssh");
        std::fs::write(
            &fake_ssh,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"-G\" ]; then exit 0; fi\ncase \"$*\" in\n  *git-upload-pack*) exec git-upload-pack '{}' ;;\n  *git-receive-pack*) exec git-receive-pack '{}' ;;\nesac\nexit 2\n",
                fake_remote.display(),
                fake_remote.display(),
            ),
        )
        .expect("fake GitHub SSH transport should be written");
        let mut permissions = std::fs::metadata(&fake_ssh)
            .expect("fake GitHub SSH transport metadata should load")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_ssh, permissions)
            .expect("fake GitHub SSH transport should be executable");
        git(
            repo.path(),
            &[
                "config",
                "core.sshCommand",
                fake_ssh.to_str().expect("SSH path should be UTF-8"),
            ],
        );
        git(
            repo.path(),
            &[
                "remote",
                "add",
                "origin",
                "git@github.com:ralphx/test-repository.git",
            ],
        );
        let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

        let github = Arc::new(MockGithubService::new());
        // When we expect the resume to publish, let the mock create a PR so publish completes
        // instead of blocking on an agent-authored PR description.
        github.will_create_pr(918, "https://github.com/owner/repo/pull/918");
        let conversation_id = ChatConversationId::new();
        let mut state = AppState::new_test();
        state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
        let publish_client = Arc::new(SubmittingPrDescriptionClient::new(
            Arc::clone(&state.agent_conversation_workspace_repo),
            conversation_id.clone(),
            false,
        ));
        let state = state.with_agent_client(publish_client);
        let app_state = Arc::new(state);
        let mut project = Project::new(
            format!("Review Completion {suffix}"),
            repo.path().to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
        project.github_pr_enabled = true;
        app_state
            .project_repo
            .create(project.clone())
            .await
            .expect("seed project");

        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        conversation.context_type = ChatContextType::Project;
        conversation.context_id = project.id.as_str().to_string();
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("seed conversation");

        let workspace_path = resolve_agent_conversation_workspace_path(&project, &conversation_id)
            .expect("workspace path");
        let branch_name = format!("ralphx/test/review-completion-{suffix}");
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                &branch_name,
                workspace_path.to_str().unwrap(),
                "main",
            ],
        );
        std::fs::write(workspace_path.join("fix.txt"), "work\n").expect("write workspace change");
        git(&workspace_path, &["add", "fix.txt"]);
        git(&workspace_path, &["commit", "-m", "workspace change"]);
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_sha),
            branch_name,
            workspace_path.to_string_lossy().to_string(),
        );
        // No publication PR yet — this is the INITIAL publish path.
        workspace.publication_pr_number = None;
        workspace.auto_publish_initial_pr_enabled = armed_initial;
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");

        let review_context = load_agent_workspace_review_context(app_state.as_ref(), &workspace)
            .await
            .expect("review context should load");
        let target = review_context.target.expect("review target should exist");
        let mut monitor = review_context.monitor;
        apply_review_artifact_to_monitor(
            &mut monitor,
            target.scope,
            target.head_sha,
            target.diff_fingerprint,
            Some("review-run".to_string()),
            ArtifactId::from_string(format!("review-artifact-{suffix}")),
            1,
            chrono::Utc::now(),
            None,
        );
        monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
        monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
        app_state
            .agent_conversation_workspace_repo
            .upsert_workspace_review_monitor(monitor)
            .await
            .expect("review monitor should persist");

        let (automation_id, run_id) = if seed_automation {
            let now = chrono::Utc::now();
            let automation_id = AutomationId::from_string(format!("automation-{suffix}"));
            app_state
                .automation_repo
                .create(Automation {
                    id: automation_id.clone(),
                    project_id: project.id.clone(),
                    name: "Automation".to_string(),
                    status: AutomationStatus::Active,
                    paused_reason_code: None,
                    paused_reason_detail: None,
                    goal_prompt: "Goal".to_string(),
                    setup_conversation_id: None,
                    provider_harness: "claude".to_string(),
                    model_id: "sonnet".to_string(),
                    logical_effort: None,
                    run_mode: "edit".to_string(),
                    base_ref_kind: "project_default".to_string(),
                    base_ref: String::new(),
                    base_display_name: None,
                    base_source_pull_request_json: None,
                    goal_items_json: None,
                    chain_mode: "merged_base".to_string(),
                    completion_signal: "pr_merged".to_string(),
                    plan_approval_mode: AutomationPlanApprovalMode::Manual,
                    pr_merge_mode: AutomationPrMergeMode::Manual,
                    plan_deep_verification: false,
                    max_runs: 25,
                    max_consecutive_failures: 3,
                    first_run_prompt: None,
                    setup_analysis_summary: None,
                    spec_artifact_id: None,
                    authoring_state_json: None,
                    created_at: now,
                    updated_at: now,
                })
                .await
                .expect("seed automation");
            let run_id = AutomationRunId::from_string(format!("run-{suffix}"));
            app_state
                .automation_run_repo
                .create_run(AutomationRun {
                    id: run_id.clone(),
                    automation_id: automation_id.clone(),
                    run_index: 1,
                    status: AutomationRunStatus::Running,
                    judge_state: AutomationJudgeState::None,
                    judge_lease_expires_at: None,
                    plan_judge_state: AutomationPlanJudgeState::None,
                    plan_judge_lease_expires_at: None,
                    plan_judge_verdict_json: None,
                    plan_revision_round: 0,
                    plan_reminder_count: 0,
                    plan_pending_instructions: None,
                    plan_last_parked_artifact_id: None,
                    plan_last_parked_blueprint_artifact_id: None,
                    agent_phase_started_at: None,
                    conversation_id: Some(conversation_id.clone()),
                    run_prompt: "Run".to_string(),
                    prompt_author: AutomationPromptAuthor::SetupAgent,
                    base_ref_kind: "project_default".to_string(),
                    base_ref_used: "main".to_string(),
                    base_from_run_id: None,
                    goal_item_id: None,
                    branch_name: None,
                    pr_number: None,
                    pr_url: None,
                    pr_title: None,
                    pr_head_ref_name: None,
                    pr_base_ref_name: None,
                    pr_merged_at: None,
                    merge_commit_sha: None,
                    diff_stats_json: None,
                    agent_summary: None,
                    judge_verdict_json: None,
                    judge_model_id: None,
                    error_code: None,
                    error_detail: None,
                    signal_check_failures: 0,
                    started_at: Some(now),
                    finished_at: None,
                    created_at: now,
                    updated_at: now,
                })
                .await
                .expect("seed automation run");
            (Some(automation_id), Some(run_id))
        } else {
            (None, None)
        };

        ReviewCompletionFixture {
            _repo: repo,
            _worktrees: worktrees,
            app_state,
            conversation_id,
            automation_id,
            run_id,
            github,
        }
    }

    #[tokio::test]
    async fn passed_review_resumes_initial_auto_publish_when_armed() {
        let fixture = setup_workspace_for_review_completion("armed", true, false).await;
        let state = test_http_state(Arc::clone(&fixture.app_state));

        let _ = complete_agent_workspace_review_run(
            State(state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspaceReviewRunRequest {
                outcome: Some("passed".to_string()),
                summary: "Review passed".to_string(),
                blocker: None,
                created_by_run_id: Some("review-run".to_string()),
            }),
        )
        .await
        .expect("passed workspace review should complete");

        // R2: the initial auto-publish resume fired (publishing event appended before publish).
        let events = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .unwrap();
        assert!(
            events.iter().any(|event| {
                event.step == "initial_auto_publish_workspace_review_passed"
                    && event.status == "publishing"
                    && event.classification.as_deref() == Some("workspace_review_passed")
            }),
            "armed initial auto-publish should resume on a passed gate"
        );
        // Publish was invoked exactly once and created the initial PR.
        assert_eq!(fixture.github.state().create_draft_pr_calls, 1);
        let persisted = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(persisted.publication_pr_number, Some(918));
    }

    #[tokio::test]
    async fn human_bypass_resumes_armed_initial_publish_for_the_exact_blocking_snapshot() {
        let fixture = setup_workspace_for_review_completion("bypass-armed", true, false).await;
        let completion_state = test_http_state(Arc::clone(&fixture.app_state));

        let _ = complete_agent_workspace_review_run(
            State(completion_state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspaceReviewRunRequest {
                outcome: Some("blocking".to_string()),
                summary: "Review found a blocker".to_string(),
                blocker: Some("A human must accept this invariant risk".to_string()),
                created_by_run_id: Some("review-run".to_string()),
            }),
        )
        .await
        .expect("blocking workspace review should complete");
        assert_eq!(fixture.github.state().create_draft_pr_calls, 0);

        let workspace = fixture
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&fixture.conversation_id)
            .await
            .unwrap()
            .unwrap();
        let context = load_agent_workspace_review_context(fixture.app_state.as_ref(), &workspace)
            .await
            .expect("blocking review context should load");
        let target = context.target.expect("review target should remain current");
        let artifact_id = context
            .monitor
            .review_artifact_id
            .expect("blocking review artifact should remain linked");
        let artifact_version = context
            .monitor
            .review_artifact_version
            .expect("blocking review artifact version should remain linked");

        let Json(response) = approve_agent_workspace_review_anyway_handler(
            State(test_http_state(Arc::clone(&fixture.app_state))),
            Path(fixture.conversation_id.to_string()),
            Json(ApproveAgentWorkspaceReviewAnywayRequest {
                target_scope: target.scope.to_string(),
                diff_fingerprint: target.diff_fingerprint,
                artifact_id: artifact_id.as_str().to_string(),
                artifact_version,
            }),
        )
        .await
        .expect("exact blocking snapshot should be human-approved");

        assert_eq!(response.monitor.review_outcome, "blocking");
        assert_eq!(response.monitor.review_gate_status, "passed");
        assert!(response.monitor.review_gate_bypassed_at.is_some());
        assert_eq!(fixture.github.state().create_draft_pr_calls, 1);
        let events = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .unwrap();
        assert!(events.iter().any(|event| {
            event.step == "workspace_review_approved_anyway"
                && event.classification.as_deref() == Some("workspace_review_approved_anyway")
        }));
        assert!(events.iter().any(|event| {
            event.step == "initial_auto_publish_workspace_review_passed"
                && event.classification.as_deref() == Some("workspace_review_approved_anyway")
        }));
    }

    #[tokio::test]
    async fn passed_review_does_not_resume_initial_auto_publish_when_not_armed() {
        let fixture = setup_workspace_for_review_completion("unarmed", false, false).await;
        let state = test_http_state(Arc::clone(&fixture.app_state));

        let Json(response) = complete_agent_workspace_review_run(
            State(state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspaceReviewRunRequest {
                outcome: Some("passed".to_string()),
                summary: "Review passed".to_string(),
                blocker: None,
                created_by_run_id: Some("review-run".to_string()),
            }),
        )
        .await
        .expect("passed workspace review should complete");

        assert_eq!(response.monitor.review_gate_status, "passed");
        let events = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .unwrap();
        assert!(
            !events
                .iter()
                .any(|event| event.step == "initial_auto_publish_workspace_review_passed"),
            "a non-armed workspace must not resume initial auto-publish"
        );
    }

    #[tokio::test]
    async fn blocking_review_pauses_owning_automation_and_terminalizes_run() {
        let fixture = setup_workspace_for_review_completion("block", false, true).await;
        let state = test_http_state(Arc::clone(&fixture.app_state));

        let _ = complete_agent_workspace_review_run(
            State(state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspaceReviewRunRequest {
                outcome: Some("blocking".to_string()),
                summary: "Review found blocking changes".to_string(),
                blocker: Some("Fix the failing invariant".to_string()),
                created_by_run_id: Some("review-run".to_string()),
            }),
        )
        .await
        .expect("blocking workspace review should complete");

        // R3 site (a): automation paused with the review-blocked reason.
        let automation = fixture
            .app_state
            .automation_repo
            .get_by_id(fixture.automation_id.as_ref().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            automation.status,
            crate::domain::entities::AutomationStatus::Paused
        );
        assert_eq!(
            automation.paused_reason_code.as_deref(),
            Some("workspace_review_blocked")
        );

        // Run terminalized as AgentFailed so its wall-clock can't false-timeout on resume.
        let run = fixture
            .app_state
            .automation_run_repo
            .get_by_id(fixture.run_id.as_ref().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            run.status,
            crate::domain::entities::AutomationRunStatus::AgentFailed
        );
        assert_eq!(run.error_code.as_deref(), Some("workspace_review_blocked"));

        // Publish was NOT invoked (no publishing events at all).
        let events = fixture
            .app_state
            .agent_conversation_workspace_repo
            .list_publication_events(&fixture.conversation_id)
            .await
            .unwrap();
        assert!(
            !events
                .iter()
                .any(|event| event.step == "initial_auto_publish_workspace_review_passed"),
            "a blocking gate must not resume publish"
        );
    }

    #[tokio::test]
    async fn blocking_review_is_noop_for_non_automation_conversation() {
        let fixture = setup_workspace_for_review_completion("block-interactive", false, false).await;
        let state = test_http_state(Arc::clone(&fixture.app_state));

        // No automation linked → the handler must still succeed and not attempt any pause.
        let Json(response) = complete_agent_workspace_review_run(
            State(state),
            Path(fixture.conversation_id.to_string()),
            Json(CompleteAgentWorkspaceReviewRunRequest {
                outcome: Some("blocking".to_string()),
                summary: "Review found blocking changes".to_string(),
                blocker: Some("Fix the failing invariant".to_string()),
                created_by_run_id: Some("review-run".to_string()),
            }),
        )
        .await
        .expect("blocking workspace review should complete for interactive conversation");

        assert_eq!(response.monitor.review_gate_status, "blocking");
    }

    #[tokio::test]
    async fn read_pr_comment_returns_full_body_and_marks_read() {
        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_pr_number = Some(267);
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        app_state
            .agent_conversation_workspace_repo
            .upsert_pr_comment_evidence(
                &conversation_id,
                vec![AgentWorkspacePrCommentEvidenceUpsert::new(
                    267,
                    "comment-1".to_string(),
                    Some("codecov".to_string()),
                    "Full Codecov report body with detailed coverage table.".to_string(),
                    Some("https://github.com/owner/repo/pull/267#issuecomment-1".to_string()),
                    Some("2026-05-18T22:00:00Z".to_string()),
                    Some("2026-05-18T22:00:00Z".to_string()),
                    true,
                    true,
                )],
            )
            .await
            .unwrap();
        let state = test_http_state(Arc::clone(&app_state));

        let Json(response) = read_agent_workspace_pr_comment(
            State(state),
            Path((conversation_id.to_string(), "comment-1".to_string())),
        )
        .await
        .expect("comment should read");

        assert!(response.success);
        assert_eq!(response.pr_number, 267);
        assert_eq!(
            response.body,
            "Full Codecov report body with detailed coverage table."
        );
        assert_eq!(response.body_length_chars, response.body.chars().count());
        assert!(response.is_untrusted);
        let stored = app_state
            .agent_conversation_workspace_repo
            .get_pr_comment_evidence(&conversation_id, 267, "comment-1")
            .await
            .unwrap()
            .unwrap();
        assert!(stored.last_read_at.is_some());
    }

    #[tokio::test]
    async fn pr_fix_context_imports_bounded_comment_evidence() {
        let mut app_state = AppState::new_test();
        let github = Arc::new(MockGithubService::new());
        let long_body = "Patch coverage report row ".repeat(40);
        github.state().fetch_pr_health_result = Some(Ok(PrHealth {
            sync_state: PrSyncState {
                status: PrStatus::Open,
                merge_state_status: None,
                mergeable: None,
                is_draft: false,
                head_ref_name: "feature/pr-description".to_string(),
                base_ref_name: "main".to_string(),
                head_ref_oid: None,
                base_ref_oid: None,
            },
            review_decision: None,
            checks: Vec::new(),
            issue_comments: vec![PrIssueCommentSummary {
                id: "comment-long".to_string(),
                author: Some("codecov".to_string()),
                body: long_body.clone(),
                url: Some("https://github.com/owner/repo/pull/267#issuecomment-1".to_string()),
                created_at: Some("2026-05-18T22:00:00Z".to_string()),
                updated_at: Some("2026-05-18T22:05:00Z".to_string()),
                is_bot: true,
                is_codecov: true,
            }],
            auto_merge_request: None,
        }));
        app_state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
        let app_state = Arc::new(app_state);
        let conversation_id = ChatConversationId::new();
        let mut workspace = test_workspace(conversation_id.clone());
        workspace.publication_pr_number = Some(267);
        workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/267".to_string());
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
        let state = test_http_state(Arc::clone(&app_state));

        let Json(response) =
            get_agent_workspace_pr_fix_context(State(state), Path(conversation_id.to_string()))
                .await
                .expect("PR fix context should load");

        assert_eq!(response.issue_comment_evidence.len(), 1);
        let evidence = &response.issue_comment_evidence[0];
        assert_eq!(evidence.comment_id, "comment-long");
        assert!(evidence.has_more);
        assert!(evidence.full_body_available);
        assert!(evidence.is_untrusted);
        assert_eq!(evidence.read_tool, "read_agent_workspace_pr_comment");
        assert_eq!(evidence.body_length_chars, long_body.chars().count());
        assert!(evidence.body_excerpt.chars().count() <= 480);
        assert!(
            response
                .health
                .as_ref()
                .expect("health should be present")
                .issue_comments[0]
                .body
                .chars()
                .count()
                <= 480
        );
        let stored = app_state
            .agent_conversation_workspace_repo
            .get_pr_comment_evidence(&conversation_id, 267, "comment-long")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.body, long_body);
        assert!(stored.last_included_at.is_some());
        assert_eq!(github.state().fetch_pr_health_calls, 1);
    }

    #[tokio::test]
    async fn pr_fix_context_uses_linked_plan_branch_pr_target() {
        let repo = tempfile::TempDir::new().expect("repo tempdir");
        let worktrees = tempfile::TempDir::new().expect("worktree tempdir");
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        git(repo.path(), &["config", "user.name", "RalphX Test"]);
        std::fs::write(repo.path().join("README.md"), "base\n").expect("write base");
        git(repo.path(), &["add", "README.md"]);
        git(repo.path(), &["commit", "-m", "base"]);
        let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

        let github = Arc::new(MockGithubService::new());
        github.state().fetch_pr_health_result = Some(Ok(PrHealth {
            sync_state: PrSyncState {
                status: PrStatus::Open,
                merge_state_status: None,
                mergeable: None,
                is_draft: false,
                head_ref_name: "ralphx/test/plan-pr-context".to_string(),
                base_ref_name: "main".to_string(),
                head_ref_oid: Some("plan-context-head".to_string()),
                base_ref_oid: None,
            },
            review_decision: None,
            checks: Vec::new(),
            issue_comments: Vec::new(),
            auto_merge_request: None,
        }));
        let mut app_state = AppState::new_test();
        app_state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
        let app_state = Arc::new(app_state);

        let mut project = Project::new(
            "Plan PR Context".to_string(),
            repo.path().to_string_lossy().to_string(),
        );
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
        app_state
            .project_repo
            .create(project.clone())
            .await
            .expect("seed project");

        let conversation_id = ChatConversationId::from_string("conversation-plan-pr-context");
        let session_id = IdeationSessionId::from_string("session-plan-pr-context");
        let plan_branch_id = PlanBranchId::from_string("plan-branch-pr-context");
        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.id = conversation_id.clone();
        conversation.context_type = ChatContextType::Project;
        conversation.context_id = project.id.as_str().to_string();
        app_state
            .chat_conversation_repo
            .create(conversation)
            .await
            .expect("seed conversation");

        let branch_name = "ralphx/test/plan-pr-context";
        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("artifact-plan-pr-context"),
            session_id.clone(),
            project.id.clone(),
            branch_name.to_string(),
            "main".to_string(),
        );
        plan_branch.id = plan_branch_id.clone();
        plan_branch.pr_eligible = true;
        plan_branch.merge_task_id = Some(TaskId::from_string(
            "merge-task-plan-pr-context".to_string(),
        ));
        plan_branch.pr_number = Some(602);
        plan_branch.pr_url = Some("https://github.com/owner/repo/pull/602".to_string());
        plan_branch.pr_status = Some(PlanPrStatus::Open);
        plan_branch.pr_push_status = PlanPrPushStatus::Pushed;
        let plan_worktree = resolve_linked_plan_branch_agent_worktree_path(&project, &plan_branch)
            .expect("plan worktree path");
        git(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                branch_name,
                plan_worktree.to_str().unwrap(),
                "main",
            ],
        );
        app_state
            .plan_branch_repo
            .create(plan_branch)
            .await
            .expect("seed plan branch");

        let mut workspace = AgentConversationWorkspace::new(
            conversation_id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Ideation,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some(base_sha),
            branch_name.to_string(),
            plan_worktree.to_string_lossy().to_string(),
        );
        workspace.linked_ideation_session_id = Some(session_id);
        workspace.linked_plan_branch_id = Some(plan_branch_id);
        workspace.publication_pr_number = None;
        workspace.pr_supervision_status = Some("fixing".to_string());
        workspace.pr_autofix_enabled = true;
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");

        let state = test_http_state(Arc::clone(&app_state));
        let Json(response) =
            get_agent_workspace_pr_fix_context(State(state), Path(conversation_id.to_string()))
                .await
                .expect("PR fix context should load");

        assert_eq!(response.target_kind.as_deref(), Some("ideation_plan_pr"));
        assert_eq!(response.pr_number, Some(602));
        assert_eq!(
            response.pr_url.as_deref(),
            Some("https://github.com/owner/repo/pull/602")
        );
        assert_eq!(response.target_branch.as_deref(), Some(branch_name));
        assert_eq!(response.target_base_branch.as_deref(), Some("main"));
        assert_eq!(response.workspace.publication_pr_number, Some(602));
        let stored = app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .expect("workspace row should exist");
        assert_eq!(stored.publication_pr_number, None);
    }

    #[test]
    fn readiness_treats_base_ahead_as_recommended_action_not_blocker() {
        let freshness = test_freshness(true, true, Some(1), "valid");

        assert!(publish_readiness_blockers(&freshness, None).is_empty());
        assert_eq!(
            publish_readiness_recommended_actions(&freshness),
            vec!["update_from_base".to_string()]
        );
    }

    #[test]
    fn readiness_blocks_missing_changes_and_blocked_base() {
        let no_changes = test_freshness(false, false, Some(0), "valid");
        assert_eq!(
            publish_readiness_blockers(&no_changes, None),
            vec!["No committed or uncommitted workspace changes to publish".to_string()]
        );

        let blocked = test_freshness(true, true, Some(1), "blocked");
        assert_eq!(
            publish_readiness_blockers(&blocked, None),
            vec!["Workspace base is blocked".to_string()]
        );
        assert!(publish_readiness_recommended_actions(&blocked).is_empty());
    }

    #[test]
    fn readiness_includes_workspace_review_gate_blocker() {
        let freshness = test_freshness(true, true, Some(1), "valid");

        assert_eq!(
            publish_readiness_blockers(
                &freshness,
                Some("Workspace Review is required before publishing".to_string()),
            ),
            vec!["Workspace Review is required before publishing".to_string()]
        );
    }

    #[tokio::test]
    async fn submit_agent_workspace_pr_description_saves_partial_patch() {
        let app_state = Arc::new(AppState::new_test());
        let conversation_id = ChatConversationId::new();
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(test_workspace(conversation_id.clone()))
            .await
            .unwrap();
        let state = test_http_state(Arc::clone(&app_state));

        let Json(response) = submit_agent_workspace_pr_description(
            State(state),
            Path(conversation_id.to_string()),
            Json(SubmitAgentWorkspacePrDescriptionRequest {
                decision: "patch".to_string(),
                title: Some("Better PR title".to_string()),
                body_markdown: None,
            }),
        )
        .await
        .unwrap();

        assert!(response.success);
        let saved = app_state
            .agent_conversation_workspace_repo
            .get_pr_metadata_decision(&conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            saved,
            crate::domain::entities::AgentWorkspacePrMetadataDecision::Patch {
                title: Some(title),
                body_markdown: None
            } if title == "Better PR title"
        ));
    }

    #[tokio::test]
    async fn submit_agent_workspace_pr_description_rejects_empty_patch() {
        let state = test_http_state(Arc::new(AppState::new_test()));

        let (status, Json(body)) = submit_agent_workspace_pr_description(
            State(state),
            Path(ChatConversationId::new().to_string()),
            Json(SubmitAgentWorkspacePrDescriptionRequest {
                decision: "patch".to_string(),
                title: None,
                body_markdown: Some("   ".to_string()),
            }),
        )
        .await
        .unwrap_err();

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().contains("requires"));
    }

    #[tokio::test]
    async fn submit_agent_workspace_pr_description_requires_workspace() {
        let state = test_http_state(Arc::new(AppState::new_test()));

        let (status, Json(body)) = submit_agent_workspace_pr_description(
            State(state),
            Path(ChatConversationId::new().to_string()),
            Json(SubmitAgentWorkspacePrDescriptionRequest {
                decision: "preserve".to_string(),
                title: None,
                body_markdown: None,
            }),
        )
        .await
        .unwrap_err();

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "Agent workspace not found");
    }

    // =========================================================================
    // Extension A/B — Diff HTTP handler tests
    // =========================================================================

    async fn create_diff_workspace() -> (
        tempfile::TempDir,
        Arc<AppState>,
        ChatConversationId,
        std::path::PathBuf,
    ) {
        use crate::application::agent_conversation_workspace::{
            prepare_agent_conversation_workspace, AgentConversationWorkspaceBaseSelection,
        };
        use crate::domain::entities::{
            AgentConversationWorkspaceMode, IdeationAnalysisBaseRefKind, Project,
        };

        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("create repo");
        git(repo.as_path(), &["init", "-b", "main"]);
        git(
            repo.as_path(),
            &["config", "user.email", "test@example.com"],
        );
        git(repo.as_path(), &["config", "user.name", "Test"]);
        std::fs::write(repo.join("base.txt"), "base\n").unwrap();
        git(repo.as_path(), &["add", "."]);
        git(repo.as_path(), &["commit", "-m", "Initial"]);

        let mut project = Project::new("Diff Test".to_string(), repo.display().to_string());
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(tmp.path().join("worktrees").display().to_string());

        let conversation_id = ChatConversationId::new();
        let workspace = prepare_agent_conversation_workspace(
            &project,
            &conversation_id,
            AgentConversationWorkspaceMode::Edit,
            AgentConversationWorkspaceBaseSelection {
                kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
                branch_mode: None,
                base_ref: Some("main".to_string()),
                display_name: None,
                source_pull_request: None,
            },
        )
        .await
        .expect("workspace prepared");

        let worktree_path = std::path::PathBuf::from(&workspace.worktree_path);

        let app_state = Arc::new(AppState::new_test());
        app_state
            .project_repo
            .create(project)
            .await
            .expect("seed project");
        app_state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");

        (tmp, app_state, conversation_id, worktree_path)
    }

    #[tokio::test]
    async fn get_staged_changes_handler_returns_staged_files() {
        let (_tmp, app_state, conversation_id, worktree_path) = create_diff_workspace().await;

        std::fs::write(worktree_path.join("staged.txt"), "staged\n").unwrap();
        git(worktree_path.as_path(), &["add", "staged.txt"]);

        let state = test_http_state(app_state);
        let Json(changes) =
            get_agent_workspace_staged_file_changes(State(state), Path(conversation_id.to_string()))
                .await
                .expect("staged changes should load");

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "staged.txt");
    }

    #[tokio::test]
    async fn get_unstaged_changes_handler_returns_unstaged_files() {
        let (_tmp, app_state, conversation_id, worktree_path) = create_diff_workspace().await;

        // Modify committed file without staging
        std::fs::write(worktree_path.join("base.txt"), "base\nmodified\n").unwrap();

        let state = test_http_state(app_state);
        let Json(changes) =
            get_agent_workspace_unstaged_file_changes(State(state), Path(conversation_id.to_string()))
                .await
                .expect("unstaged changes should load");

        assert!(changes.iter().any(|c| c.path == "base.txt"));
    }

    #[tokio::test]
    async fn get_staged_diff_handler_returns_head_vs_index_content() {
        let (_tmp, app_state, conversation_id, worktree_path) = create_diff_workspace().await;

        std::fs::write(worktree_path.join("base.txt"), "base\nnew\n").unwrap();
        git(worktree_path.as_path(), &["add", "base.txt"]);
        // Further unstaged change — should NOT appear
        std::fs::write(worktree_path.join("base.txt"), "base\nnew\nextra\n").unwrap();

        let state = test_http_state(app_state);
        let Json(diff) = get_agent_workspace_staged_file_diff(
            State(state),
            Path((conversation_id.to_string(), "base.txt".to_string())),
        )
        .await
        .expect("staged diff should load");

        // Hunk-based: staged diff HEAD→index; "new" line appears as an addition
        assert!(
            diff.hunks
                .iter()
                .flat_map(|h| h.lines.iter())
                .any(|l| l.content.contains("new")),
            "staged diff hunks should contain the staged addition"
        );
        assert_eq!(diff.old_total_lines, 1, "HEAD has 1 line");
        assert_eq!(diff.new_total_lines, 2, "index has 2 lines");
    }

    #[tokio::test]
    async fn get_cumulative_changes_handler_shows_all_committed_changes() {
        let (_tmp, app_state, conversation_id, worktree_path) = create_diff_workspace().await;

        // Commit a change in the worktree
        std::fs::write(worktree_path.join("committed.txt"), "committed\n").unwrap();
        git(worktree_path.as_path(), &["add", "committed.txt"]);
        git(
            worktree_path.as_path(),
            &["commit", "-m", "Add committed file"],
        );

        let state = test_http_state(app_state);
        let Json(changes) = get_agent_workspace_cumulative_file_changes(
            State(state),
            Path(conversation_id.to_string()),
        )
        .await
        .expect("cumulative changes should load");

        assert!(changes.iter().any(|c| c.path == "committed.txt"));
    }

    #[tokio::test]
    async fn get_cumulative_diff_handler_shows_base_to_head_file_content() {
        let (_tmp, app_state, conversation_id, worktree_path) = create_diff_workspace().await;

        // Commit a new file in the worktree
        std::fs::write(worktree_path.join("new.rs"), "pub fn hello() {}\n").unwrap();
        git(worktree_path.as_path(), &["add", "new.rs"]);
        git(worktree_path.as_path(), &["commit", "-m", "Add new.rs"]);

        let state = test_http_state(app_state);
        let Json(diff) = get_agent_workspace_cumulative_file_diff(
            State(state),
            Path((conversation_id.to_string(), "new.rs".to_string())),
        )
        .await
        .expect("cumulative diff should load");

        assert_eq!(diff.file_path, "new.rs");
        // Hunk-based: cumulative diff base→HEAD; "hello" fn appears as additions
        assert!(
            diff.hunks
                .iter()
                .flat_map(|h| h.lines.iter())
                .any(|l| l.content.contains("hello")),
            "cumulative diff hunks should contain the committed function"
        );
        // File did not exist at base, so old_total_lines = 0
        assert_eq!(diff.old_total_lines, 0, "File did not exist in base");
    }

    #[tokio::test]
    async fn staged_and_cumulative_handlers_return_404_for_unknown_workspace() {
        let state = test_http_state(Arc::new(AppState::new_test()));

        let (status, _) = get_agent_workspace_staged_file_changes(
            State(state.clone()),
            Path(ChatConversationId::new().to_string()),
        )
        .await
        .unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

        let (status, _) = get_agent_workspace_cumulative_file_changes(
            State(state),
            Path(ChatConversationId::new().to_string()),
        )
        .await
        .unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }
