use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};
use tauri::State;

use crate::application::AppState;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    processed_tokens, AgentRun, AgentRunUsage, ChatConversation, ChatMessage, MessageRole,
    UsageProvenance,
};

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageTotalsResponse {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub processed_tokens: Option<u64>,
    pub estimated_usd: Option<f64>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageBucketResponse {
    pub key: String,
    pub count: u64,
    pub usage: UsageTotalsResponse,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationUsageCoverageResponse {
    pub provider_message_count: u64,
    pub provider_messages_with_usage: u64,
    pub run_count: u64,
    pub runs_with_usage: u64,
    pub effective_run_conversation_count: u64,
    pub effective_message_conversation_count: u64,
    pub legacy_estimated_sample_count: u64,
    pub fallback_estimated_sample_count: u64,
    pub uncounted_sample_count: u64,
    pub effective_totals_source: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationAttributionCoverageResponse {
    pub provider_message_count: u64,
    pub provider_messages_with_attribution: u64,
    pub run_count: u64,
    pub runs_with_attribution: u64,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationStatsResponse {
    pub conversation_id: String,
    pub context_type: String,
    pub context_id: String,
    pub provider_harness: Option<String>,
    pub upstream_provider: Option<String>,
    pub provider_profile: Option<String>,
    pub message_usage_totals: UsageTotalsResponse,
    pub run_usage_totals: UsageTotalsResponse,
    pub effective_usage_totals: UsageTotalsResponse,
    pub usage_coverage: ConversationUsageCoverageResponse,
    pub attribution_coverage: ConversationAttributionCoverageResponse,
    pub by_harness: Vec<UsageBucketResponse>,
    pub by_upstream_provider: Vec<UsageBucketResponse>,
    pub by_model: Vec<UsageBucketResponse>,
    pub by_effort: Vec<UsageBucketResponse>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScopeStatsResponse {
    pub scope_type: String,
    pub scope_id: String,
    pub conversation_count: u64,
    pub message_usage_totals: UsageTotalsResponse,
    pub run_usage_totals: UsageTotalsResponse,
    pub effective_usage_totals: UsageTotalsResponse,
    pub usage_coverage: ConversationUsageCoverageResponse,
    pub attribution_coverage: ConversationAttributionCoverageResponse,
    pub by_context_type: Vec<UsageBucketResponse>,
    pub by_harness: Vec<UsageBucketResponse>,
    pub by_upstream_provider: Vec<UsageBucketResponse>,
    pub by_model: Vec<UsageBucketResponse>,
    pub by_effort: Vec<UsageBucketResponse>,
}

pub fn build_conversation_stats_response(
    conversation: &ChatConversation,
    messages: &[ChatMessage],
    runs: &[AgentRun],
) -> ConversationStatsResponse {
    let aggregates = build_usage_aggregates(std::slice::from_ref(conversation), messages, runs);

    ConversationStatsResponse {
        conversation_id: conversation.id.as_str(),
        context_type: conversation.context_type.to_string(),
        context_id: conversation.context_id.clone(),
        provider_harness: conversation.provider_harness.map(|value| value.to_string()),
        upstream_provider: conversation.upstream_provider.clone(),
        provider_profile: conversation.provider_profile.clone(),
        message_usage_totals: aggregates.message_usage_totals,
        run_usage_totals: aggregates.run_usage_totals,
        effective_usage_totals: aggregates.effective_usage_totals,
        usage_coverage: aggregates.usage_coverage,
        attribution_coverage: aggregates.attribution_coverage,
        by_harness: aggregates.by_harness,
        by_upstream_provider: aggregates.by_upstream_provider,
        by_model: aggregates.by_model,
        by_effort: aggregates.by_effort,
    }
}

pub fn build_scope_stats_response(
    scope_type: &str,
    scope_id: &str,
    conversations: &[ChatConversation],
    messages: &[ChatMessage],
    runs: &[AgentRun],
) -> ScopeStatsResponse {
    let aggregates = build_usage_aggregates(conversations, messages, runs);

    ScopeStatsResponse {
        scope_type: scope_type.to_string(),
        scope_id: scope_id.to_string(),
        conversation_count: conversations.len() as u64,
        message_usage_totals: aggregates.message_usage_totals,
        run_usage_totals: aggregates.run_usage_totals,
        effective_usage_totals: aggregates.effective_usage_totals,
        usage_coverage: aggregates.usage_coverage,
        attribution_coverage: aggregates.attribution_coverage,
        by_context_type: aggregates.by_context_type,
        by_harness: aggregates.by_harness,
        by_upstream_provider: aggregates.by_upstream_provider,
        by_model: aggregates.by_model,
        by_effort: aggregates.by_effort,
    }
}

#[derive(Debug, Clone)]
struct UsageAggregateResult {
    message_usage_totals: UsageTotalsResponse,
    run_usage_totals: UsageTotalsResponse,
    effective_usage_totals: UsageTotalsResponse,
    usage_coverage: ConversationUsageCoverageResponse,
    attribution_coverage: ConversationAttributionCoverageResponse,
    by_context_type: Vec<UsageBucketResponse>,
    by_harness: Vec<UsageBucketResponse>,
    by_upstream_provider: Vec<UsageBucketResponse>,
    by_model: Vec<UsageBucketResponse>,
    by_effort: Vec<UsageBucketResponse>,
}

fn build_usage_aggregates(
    conversations: &[ChatConversation],
    messages: &[ChatMessage],
    runs: &[AgentRun],
) -> UsageAggregateResult {
    let conversation_contexts: HashMap<_, _> = conversations
        .iter()
        .map(|conversation| (conversation.id, conversation.context_type.to_string()))
        .collect();

    let provider_messages: Vec<&ChatMessage> = messages
        .iter()
        .filter(|message| is_provider_message(message.role))
        .collect();
    let provider_messages_with_usage: Vec<&ChatMessage> = provider_messages
        .iter()
        .copied()
        .filter(message_has_usage)
        .collect();
    let provider_messages_with_attribution = provider_messages
        .iter()
        .copied()
        .filter(message_has_attribution)
        .count() as u64;

    let runs_with_usage: Vec<&AgentRun> = runs.iter().filter(run_has_usage).collect();
    let runs_with_attribution = runs.iter().filter(|run| run_has_attribution(run)).count() as u64;

    let message_samples = provider_messages_with_usage
        .iter()
        .map(|message| message_usage_sample(message, &conversation_contexts))
        .collect::<Vec<_>>();
    let run_samples = runs_with_usage
        .iter()
        .map(|run| run_usage_sample(run, &conversation_contexts))
        .collect::<Vec<_>>();
    let message_usage = resolve_usage_samples(message_samples.clone());
    let run_usage = resolve_usage_samples(run_samples.clone());
    let message_usage_totals = sum_resolved_samples(&message_usage);
    let run_usage_totals = sum_resolved_samples(&run_usage);

    let mut messages_by_conversation: HashMap<String, Vec<UsageSample>> = HashMap::new();
    for sample in message_samples {
        if let Some(conversation_id) = sample.conversation_id.as_ref() {
            messages_by_conversation
                .entry(conversation_id.clone())
                .or_default()
                .push(sample);
        }
    }
    let mut runs_by_conversation: HashMap<String, Vec<UsageSample>> = HashMap::new();
    for sample in run_samples {
        if let Some(conversation_id) = sample.conversation_id.as_ref() {
            runs_by_conversation
                .entry(conversation_id.clone())
                .or_default()
                .push(sample);
        }
    }

    let mut effective_usage = ResolvedUsage::default();
    let mut effective_run_conversation_count = 0;
    let mut effective_message_conversation_count = 0;
    for conversation in conversations {
        let conversation_id = conversation.id.as_str();
        let message_usage = resolve_usage_samples(
            messages_by_conversation
                .remove(&conversation_id)
                .unwrap_or_default(),
        );
        let run_usage = resolve_usage_samples(
            runs_by_conversation
                .remove(&conversation_id)
                .unwrap_or_default(),
        );
        if message_usage.usable_sample_count() > run_usage.usable_sample_count() {
            effective_message_conversation_count += 1;
            effective_usage.extend(message_usage);
        } else if run_usage.usable_sample_count() > 0 {
            effective_run_conversation_count += 1;
            effective_usage.extend(run_usage);
        } else if !run_usage.samples.is_empty() {
            // Preserve quality evidence for baseline-only rows without claiming a
            // usable source. The run ledger still wins an all-uncounted tie.
            effective_usage.extend(run_usage);
        } else if !message_usage.samples.is_empty() {
            effective_usage.extend(message_usage);
        }
    }

    let effective_usage_source = match (
        effective_message_conversation_count > 0,
        effective_run_conversation_count > 0,
    ) {
        (true, true) => "mixed",
        (true, false) => "messages",
        (false, true) => "runs",
        (false, false) => "none",
    };
    let effective_usage_totals = sum_resolved_samples(&effective_usage);
    let by_context_type = aggregate_usage_buckets(&effective_usage.samples, |sample| {
        sample.context_type.clone()
    });
    let by_harness = aggregate_usage_buckets(&effective_usage.samples, |sample| {
        sample.harness.map(|value| value.to_string())
    });
    let by_upstream_provider = aggregate_usage_buckets(&effective_usage.samples, |sample| {
        sample.upstream_provider.clone()
    });
    let by_model = aggregate_usage_buckets(&effective_usage.samples, |sample| {
        sample.effective_model_id.clone()
    });
    let by_effort = aggregate_usage_buckets(&effective_usage.samples, |sample| {
        sample.effective_effort.clone()
    });

    UsageAggregateResult {
        message_usage_totals,
        run_usage_totals,
        effective_usage_totals,
        usage_coverage: ConversationUsageCoverageResponse {
            provider_message_count: provider_messages.len() as u64,
            provider_messages_with_usage: provider_messages_with_usage.len() as u64,
            run_count: runs.len() as u64,
            runs_with_usage: runs_with_usage.len() as u64,
            effective_run_conversation_count,
            effective_message_conversation_count,
            legacy_estimated_sample_count: effective_usage.legacy_estimated_sample_count,
            fallback_estimated_sample_count: effective_usage.fallback_estimated_sample_count,
            uncounted_sample_count: effective_usage.uncounted_sample_count,
            effective_totals_source: effective_usage_source.to_string(),
        },
        attribution_coverage: ConversationAttributionCoverageResponse {
            provider_message_count: provider_messages.len() as u64,
            provider_messages_with_attribution,
            run_count: runs.len() as u64,
            runs_with_attribution,
        },
        by_context_type,
        by_harness,
        by_upstream_provider,
        by_model,
        by_effort,
    }
}

#[tauri::command]
pub async fn get_agent_conversation_stats(
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<Option<ConversationStatsResponse>, String> {
    let conversation_id = crate::domain::entities::ChatConversationId::from_string(conversation_id);
    let Some(conversation) = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };

    let messages = state
        .chat_message_repo
        .get_by_conversation(&conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    let runs = state
        .agent_run_repo
        .get_by_conversation(&conversation_id)
        .await
        .map_err(|error| error.to_string())?;

    Ok(Some(build_conversation_stats_response(
        &conversation,
        &messages,
        &runs,
    )))
}

#[tauri::command]
pub async fn get_project_chat_usage_stats(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<ScopeStatsResponse, String> {
    let project_id_obj = crate::domain::entities::ProjectId::from_string(project_id.clone());
    let conversations = collect_project_conversations(&state, &project_id_obj).await?;
    let (messages, runs) = collect_conversation_payloads(&state, &conversations).await?;
    Ok(build_scope_stats_response(
        "project",
        project_id_obj.as_str(),
        &conversations,
        &messages,
        &runs,
    ))
}

#[tauri::command]
pub async fn get_insights_chat_usage_stats(
    project_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<ScopeStatsResponse, String> {
    let project_id = project_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let (scope_type, scope_id, conversations) = match project_id {
        Some(project_id) => {
            let project_id_obj = crate::domain::entities::ProjectId::from_string(project_id);
            (
                "project",
                project_id_obj.as_str().to_string(),
                collect_project_conversations(&state, &project_id_obj).await?,
            )
        }
        None => (
            "all_projects",
            "all".to_string(),
            collect_all_project_conversations(&state).await?,
        ),
    };
    let (messages, runs) = collect_conversation_payloads(&state, &conversations).await?;
    Ok(build_scope_stats_response(
        scope_type,
        scope_id.as_str(),
        &conversations,
        &messages,
        &runs,
    ))
}

#[tauri::command]
pub async fn get_task_chat_usage_stats(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<ScopeStatsResponse, String> {
    let task_id_obj = crate::domain::entities::TaskId::from_string(task_id.clone());
    let conversations = collect_task_conversations(&state, &task_id_obj).await?;
    let (messages, runs) = collect_conversation_payloads(&state, &conversations).await?;
    Ok(build_scope_stats_response(
        "task",
        task_id_obj.as_str(),
        &conversations,
        &messages,
        &runs,
    ))
}

async fn collect_all_project_conversations(
    state: &AppState,
) -> Result<Vec<ChatConversation>, String> {
    let mut conversation_map: BTreeMap<String, ChatConversation> = BTreeMap::new();
    let projects = state
        .project_repo
        .get_all()
        .await
        .map_err(|error| error.to_string())?;

    for project in projects {
        for conversation in collect_project_conversations(state, &project.id).await? {
            conversation_map.insert(conversation.id.as_str(), conversation);
        }
    }

    Ok(conversation_map.into_values().collect())
}

async fn collect_project_conversations(
    state: &AppState,
    project_id: &crate::domain::entities::ProjectId,
) -> Result<Vec<ChatConversation>, String> {
    let mut conversation_map: BTreeMap<String, ChatConversation> = BTreeMap::new();
    collect_context_conversations(
        state,
        &mut conversation_map,
        crate::domain::entities::ChatContextType::Project,
        project_id.as_str(),
    )
    .await?;

    let tasks = state
        .task_repo
        .get_by_project(project_id)
        .await
        .map_err(|error| error.to_string())?;
    for task in tasks {
        collect_task_contexts(state, &mut conversation_map, &task.id).await?;
    }

    let ideation_sessions = state
        .ideation_session_repo
        .get_by_project(project_id)
        .await
        .map_err(|error| error.to_string())?;
    for session in ideation_sessions {
        collect_context_conversations(
            state,
            &mut conversation_map,
            crate::domain::entities::ChatContextType::Ideation,
            session.id.as_str(),
        )
        .await?;
    }

    Ok(conversation_map.into_values().collect())
}

async fn collect_task_conversations(
    state: &AppState,
    task_id: &crate::domain::entities::TaskId,
) -> Result<Vec<ChatConversation>, String> {
    let mut conversation_map: BTreeMap<String, ChatConversation> = BTreeMap::new();
    collect_task_contexts(state, &mut conversation_map, task_id).await?;
    Ok(conversation_map.into_values().collect())
}

async fn collect_task_contexts(
    state: &AppState,
    conversation_map: &mut BTreeMap<String, ChatConversation>,
    task_id: &crate::domain::entities::TaskId,
) -> Result<(), String> {
    for context_type in [
        crate::domain::entities::ChatContextType::Task,
        crate::domain::entities::ChatContextType::TaskExecution,
        crate::domain::entities::ChatContextType::Review,
        crate::domain::entities::ChatContextType::Merge,
    ] {
        collect_context_conversations(state, conversation_map, context_type, task_id.as_str())
            .await?;
    }
    Ok(())
}

async fn collect_context_conversations(
    state: &AppState,
    conversation_map: &mut BTreeMap<String, ChatConversation>,
    context_type: crate::domain::entities::ChatContextType,
    context_id: &str,
) -> Result<(), String> {
    let conversations = state
        .chat_conversation_repo
        .get_by_context(context_type, context_id)
        .await
        .map_err(|error| error.to_string())?;
    for conversation in conversations {
        conversation_map.insert(conversation.id.as_str(), conversation);
    }
    Ok(())
}

async fn collect_conversation_payloads(
    state: &AppState,
    conversations: &[ChatConversation],
) -> Result<(Vec<ChatMessage>, Vec<AgentRun>), String> {
    let mut messages = Vec::new();
    let mut runs = Vec::new();

    for conversation in conversations {
        messages.extend(
            state
                .chat_message_repo
                .get_by_conversation(&conversation.id)
                .await
                .map_err(|error| error.to_string())?,
        );
        runs.extend(
            state
                .agent_run_repo
                .get_by_conversation(&conversation.id)
                .await
                .map_err(|error| error.to_string())?,
        );
    }

    Ok((messages, runs))
}

#[derive(Clone)]
struct UsageAccumulator {
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_tokens: u64,
    cache_read_tokens: u64,
    processed_tokens: u64,
    processed_available: bool,
    sample_count: u64,
    estimated_usd: Option<f64>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct UsageSeriesKey {
    conversation_id: Option<String>,
    provider_session_id: Option<String>,
}

#[derive(Debug, Clone)]
struct UsageSample {
    conversation_id: Option<String>,
    context_type: Option<String>,
    harness: Option<AgentHarnessKind>,
    provider_session_id: Option<String>,
    upstream_provider: Option<String>,
    effective_model_id: Option<String>,
    effective_effort: Option<String>,
    occurred_at: DateTime<Utc>,
    usage: AgentRunUsage,
    provenance: Option<UsageProvenance>,
}

#[derive(Debug, Clone, Default)]
struct ResolvedUsage {
    samples: Vec<UsageSample>,
    usable_sample_count: usize,
    legacy_estimated_sample_count: u64,
    fallback_estimated_sample_count: u64,
    uncounted_sample_count: u64,
}

impl ResolvedUsage {
    fn usable_sample_count(&self) -> usize {
        self.usable_sample_count
    }

    fn extend(&mut self, other: Self) {
        self.samples.extend(other.samples);
        self.usable_sample_count += other.usable_sample_count;
        self.legacy_estimated_sample_count += other.legacy_estimated_sample_count;
        self.fallback_estimated_sample_count += other.fallback_estimated_sample_count;
        self.uncounted_sample_count += other.uncounted_sample_count;
    }
}

impl Default for UsageAccumulator {
    fn default() -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            processed_tokens: 0,
            processed_available: true,
            sample_count: 0,
            estimated_usd: None,
        }
    }
}

impl UsageAccumulator {
    fn add_sample(&mut self, sample: &UsageSample) {
        self.sample_count += 1;
        self.input_tokens = self
            .input_tokens
            .saturating_add(sample.usage.input_tokens.unwrap_or(0));
        self.output_tokens = self
            .output_tokens
            .saturating_add(sample.usage.output_tokens.unwrap_or(0));
        self.cache_creation_tokens = self
            .cache_creation_tokens
            .saturating_add(sample.usage.cache_creation_tokens.unwrap_or(0));
        self.cache_read_tokens = self
            .cache_read_tokens
            .saturating_add(sample.usage.cache_read_tokens.unwrap_or(0));
        if self.processed_available {
            self.processed_available =
                processed_tokens(sample.harness, &sample.usage, sample.provenance)
                    .and_then(|value| self.processed_tokens.checked_add(value))
                    .map(|total| {
                        self.processed_tokens = total;
                    })
                    .is_some();
        }
        if let Some(value) = sample.usage.estimated_usd {
            self.estimated_usd = Some(self.estimated_usd.unwrap_or(0.0) + value);
        }
    }

    fn to_response(&self) -> UsageTotalsResponse {
        UsageTotalsResponse {
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_creation_tokens: self.cache_creation_tokens,
            cache_read_tokens: self.cache_read_tokens,
            processed_tokens: (self.sample_count > 0 && self.processed_available)
                .then_some(self.processed_tokens),
            estimated_usd: self.estimated_usd,
        }
    }
}

impl Default for UsageTotalsResponse {
    fn default() -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            processed_tokens: None,
            estimated_usd: None,
        }
    }
}

fn is_provider_message(role: MessageRole) -> bool {
    !matches!(role, MessageRole::User | MessageRole::System)
}

fn message_has_usage(message: &&ChatMessage) -> bool {
    message.input_tokens.is_some()
        || message.output_tokens.is_some()
        || message.cache_creation_tokens.is_some()
        || message.cache_read_tokens.is_some()
        || message.estimated_usd.is_some()
        || message.usage_provenance.is_some()
        || message.raw_usage_snapshot.is_some()
}

fn message_has_attribution(message: &&ChatMessage) -> bool {
    message.provider_harness.is_some()
        || message.provider_session_id.is_some()
        || message.upstream_provider.is_some()
        || message.provider_profile.is_some()
        || message.effective_model_id.is_some()
        || message.effective_effort.is_some()
}

fn run_has_usage(run: &&AgentRun) -> bool {
    run.input_tokens.is_some()
        || run.output_tokens.is_some()
        || run.cache_creation_tokens.is_some()
        || run.cache_read_tokens.is_some()
        || run.estimated_usd.is_some()
        || run.usage_provenance.is_some()
        || run.raw_usage_snapshot.is_some()
}

fn run_has_attribution(run: &AgentRun) -> bool {
    run.harness.is_some()
        || run.provider_session_id.is_some()
        || run.upstream_provider.is_some()
        || run.provider_profile.is_some()
        || run.effective_model_id.is_some()
        || run.effective_effort.is_some()
}

fn message_usage_sample(
    message: &ChatMessage,
    conversation_contexts: &HashMap<crate::domain::entities::ChatConversationId, String>,
) -> UsageSample {
    UsageSample {
        conversation_id: message.conversation_id.map(|id| id.as_str()),
        context_type: message
            .conversation_id
            .and_then(|id| conversation_contexts.get(&id).cloned()),
        harness: message.provider_harness,
        provider_session_id: message.provider_session_id.clone(),
        upstream_provider: message.upstream_provider.clone(),
        effective_model_id: message.effective_model_id.clone(),
        effective_effort: message
            .effective_effort
            .clone()
            .or_else(|| message.logical_effort.map(|value| value.to_string())),
        occurred_at: message.created_at,
        usage: AgentRunUsage {
            input_tokens: message.input_tokens,
            output_tokens: message.output_tokens,
            cache_creation_tokens: message.cache_creation_tokens,
            cache_read_tokens: message.cache_read_tokens,
            estimated_usd: message.estimated_usd,
        },
        provenance: message.usage_provenance,
    }
}

fn run_usage_sample(
    run: &AgentRun,
    conversation_contexts: &HashMap<crate::domain::entities::ChatConversationId, String>,
) -> UsageSample {
    UsageSample {
        conversation_id: Some(run.conversation_id.as_str()),
        context_type: conversation_contexts.get(&run.conversation_id).cloned(),
        harness: run.harness,
        provider_session_id: run.provider_session_id.clone(),
        upstream_provider: run.upstream_provider.clone(),
        effective_model_id: run.effective_model_id.clone(),
        effective_effort: run
            .effective_effort
            .clone()
            .or_else(|| run.logical_effort.map(|value| value.to_string())),
        occurred_at: run.started_at,
        usage: AgentRunUsage {
            input_tokens: run.input_tokens,
            output_tokens: run.output_tokens,
            cache_creation_tokens: run.cache_creation_tokens,
            cache_read_tokens: run.cache_read_tokens,
            estimated_usd: run.estimated_usd,
        },
        provenance: run.usage_provenance,
    }
}

fn resolve_usage_samples(samples: Vec<UsageSample>) -> ResolvedUsage {
    let mut resolved = ResolvedUsage::default();
    let mut codex_groups: HashMap<UsageSeriesKey, Vec<UsageSample>> = HashMap::new();

    for sample in samples {
        if processed_tokens(sample.harness, &sample.usage, sample.provenance).is_some() {
            resolved.usable_sample_count += 1;
        }
        match sample.provenance {
            Some(UsageProvenance::CumulativeBaselineOnly) => {
                resolved.samples.push(sample);
            }
            Some(UsageProvenance::ProviderSnapshotFallback) => {
                resolved.fallback_estimated_sample_count += 1;
                resolved.samples.push(sample);
            }
            Some(UsageProvenance::ProviderTurnDelta | UsageProvenance::DerivedCumulativeDelta) => {
                resolved.samples.push(sample)
            }
            None if sample.harness == Some(AgentHarnessKind::Codex)
                && sample.provider_session_id.is_some() =>
            {
                codex_groups
                    .entry(UsageSeriesKey {
                        conversation_id: sample.conversation_id.clone(),
                        provider_session_id: sample.provider_session_id.clone(),
                    })
                    .or_default()
                    .push(sample);
            }
            None => {
                resolved.legacy_estimated_sample_count += 1;
                resolved.samples.push(sample);
            }
        }
    }

    for mut samples in codex_groups.into_values() {
        samples.sort_by_key(|sample| sample.occurred_at);
        resolved.legacy_estimated_sample_count += samples.len() as u64;
        if let Some(mut effective) = samples.last().cloned() {
            effective.usage = normalize_codex_stats_usage_series(&samples);
            resolved.samples.push(effective);
        }
    }

    resolved.uncounted_sample_count += resolved
        .samples
        .iter()
        .filter(|sample| {
            processed_tokens(sample.harness, &sample.usage, sample.provenance).is_none()
        })
        .count() as u64;
    resolved
}

fn sum_resolved_samples(resolved: &ResolvedUsage) -> UsageTotalsResponse {
    let mut total = sum_usage_samples(&resolved.samples);
    if resolved.uncounted_sample_count > 0 {
        total.processed_tokens = None;
    }
    total
}

fn sum_usage_samples(samples: &[UsageSample]) -> UsageTotalsResponse {
    let mut total = UsageAccumulator::default();
    for sample in samples {
        total.add_sample(sample);
    }
    total.to_response()
}

fn normalize_codex_stats_usage_series(samples: &[UsageSample]) -> AgentRunUsage {
    let cumulative = looks_like_cumulative_token_series(
        &samples
            .iter()
            .map(|sample| sample.usage.input_tokens)
            .collect::<Vec<_>>(),
    ) || looks_like_cumulative_token_series(
        &samples
            .iter()
            .map(|sample| sample.usage.output_tokens)
            .collect::<Vec<_>>(),
    ) || looks_like_cumulative_token_series(
        &samples
            .iter()
            .map(|sample| sample.usage.cache_creation_tokens)
            .collect::<Vec<_>>(),
    ) || looks_like_cumulative_token_series(
        &samples
            .iter()
            .map(|sample| sample.usage.cache_read_tokens)
            .collect::<Vec<_>>(),
    );

    let normalized = (|| {
        Ok::<AgentRunUsage, ()>(AgentRunUsage {
            input_tokens: normalize_token_series(
                &samples
                    .iter()
                    .map(|sample| sample.usage.input_tokens)
                    .collect::<Vec<_>>(),
                cumulative,
            )?,
            output_tokens: normalize_token_series(
                &samples
                    .iter()
                    .map(|sample| sample.usage.output_tokens)
                    .collect::<Vec<_>>(),
                cumulative,
            )?,
            cache_creation_tokens: normalize_token_series(
                &samples
                    .iter()
                    .map(|sample| sample.usage.cache_creation_tokens)
                    .collect::<Vec<_>>(),
                cumulative,
            )?,
            cache_read_tokens: normalize_token_series(
                &samples
                    .iter()
                    .map(|sample| sample.usage.cache_read_tokens)
                    .collect::<Vec<_>>(),
                cumulative,
            )?,
            estimated_usd: normalize_cost_series(
                &samples
                    .iter()
                    .map(|sample| sample.usage.estimated_usd)
                    .collect::<Vec<_>>(),
                cumulative,
            ),
        })
    })();

    normalized.unwrap_or_default()
}

fn looks_like_cumulative_token_series(values: &[Option<u64>]) -> bool {
    let values: Vec<u64> = values.iter().copied().flatten().collect();
    if values.len() < 3 {
        return false;
    }
    if !values.windows(2).all(|window| window[1] >= window[0]) {
        return false;
    }

    let Some(last) = values.last().copied() else {
        return false;
    };
    let Some(raw_sum) = values.iter().copied().try_fold(0_u64, u64::checked_add) else {
        return false;
    };
    let large_enough = last >= 1_000_000 || raw_sum >= 10_000_000;
    large_enough && raw_sum >= last.saturating_mul(2)
}

fn normalize_token_series(values: &[Option<u64>], cumulative: bool) -> Result<Option<u64>, ()> {
    let values: Vec<u64> = values.iter().copied().flatten().collect();
    if values.is_empty() {
        return Ok(None);
    }
    if cumulative && values.windows(2).all(|window| window[1] >= window[0]) {
        return Ok(values.last().copied());
    }
    values
        .into_iter()
        .try_fold(0_u64, u64::checked_add)
        .map(Some)
        .ok_or(())
}

fn normalize_cost_series(values: &[Option<f64>], cumulative: bool) -> Option<f64> {
    let values: Vec<f64> = values.iter().copied().flatten().collect();
    if values.is_empty() {
        return None;
    }
    if cumulative && values.windows(2).all(|window| window[1] >= window[0]) {
        return values.last().copied();
    }
    Some(values.iter().copied().sum())
}

fn aggregate_usage_buckets(
    samples: &[UsageSample],
    key_fn: impl Fn(&UsageSample) -> Option<String>,
) -> Vec<UsageBucketResponse> {
    let mut buckets: BTreeMap<String, Vec<UsageSample>> = BTreeMap::new();
    for sample in samples {
        let key = key_fn(sample).unwrap_or_else(|| "unknown".to_string());
        buckets.entry(key).or_default().push(sample.clone());
    }
    buckets
        .into_iter()
        .map(|(key, samples)| UsageBucketResponse {
            key,
            count: samples.len() as u64,
            usage: sum_usage_samples(&samples),
        })
        .collect()
}
