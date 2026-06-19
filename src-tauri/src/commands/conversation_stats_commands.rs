use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};
use tauri::State;

use crate::application::AppState;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    AgentRun, AgentRunUsage, ChatConversation, ChatMessage, MessageRole,
};

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageTotalsResponse {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
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

    let message_usage_totals = sum_message_usage(&provider_messages_with_usage);
    let run_usage_totals = sum_run_usage(&runs_with_usage);
    let effective_usage_source = if !provider_messages_with_usage.is_empty() {
        "messages"
    } else if !runs_with_usage.is_empty() {
        "runs"
    } else {
        "none"
    };

    let (
        by_context_type,
        by_harness,
        by_upstream_provider,
        by_model,
        by_effort,
        effective_usage_totals,
    ) = match effective_usage_source {
        "messages" => (
            aggregate_message_buckets(&provider_messages_with_usage, |message| {
                message.conversation_id.and_then(|conversation_id| {
                    conversation_contexts.get(&conversation_id).cloned()
                })
            }),
            aggregate_message_buckets(&provider_messages_with_usage, |message| {
                message.provider_harness.map(|value| value.to_string())
            }),
            aggregate_message_buckets(&provider_messages_with_usage, |message| {
                message.upstream_provider.clone()
            }),
            aggregate_message_buckets(&provider_messages_with_usage, |message| {
                message.effective_model_id.clone()
            }),
            aggregate_message_buckets(&provider_messages_with_usage, |message| {
                message
                    .effective_effort
                    .clone()
                    .or_else(|| message.logical_effort.map(|value| value.to_string()))
            }),
            message_usage_totals.clone(),
        ),
        "runs" => (
            aggregate_run_buckets(&runs_with_usage, |run| {
                conversation_contexts.get(&run.conversation_id).cloned()
            }),
            aggregate_run_buckets(&runs_with_usage, |run| {
                run.harness.map(|value| value.to_string())
            }),
            aggregate_run_buckets(&runs_with_usage, |run| run.upstream_provider.clone()),
            aggregate_run_buckets(&runs_with_usage, |run| run.effective_model_id.clone()),
            aggregate_run_buckets(&runs_with_usage, |run| {
                run.effective_effort
                    .clone()
                    .or_else(|| run.logical_effort.map(|value| value.to_string()))
            }),
            run_usage_totals.clone(),
        ),
        _ => (
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            UsageTotalsResponse::default(),
        ),
    };

    UsageAggregateResult {
        message_usage_totals,
        run_usage_totals,
        effective_usage_totals,
        usage_coverage: ConversationUsageCoverageResponse {
            provider_message_count: provider_messages.len() as u64,
            provider_messages_with_usage: provider_messages_with_usage.len() as u64,
            run_count: runs.len() as u64,
            runs_with_usage: runs_with_usage.len() as u64,
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

#[derive(Default, Clone)]
struct UsageAccumulator {
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_tokens: u64,
    cache_read_tokens: u64,
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
    harness: Option<AgentHarnessKind>,
    provider_session_id: Option<String>,
    occurred_at: DateTime<Utc>,
    usage: AgentRunUsage,
}

impl UsageAccumulator {
    fn add_usage(&mut self, usage: &AgentRunUsage) {
        self.input_tokens += usage.input_tokens.unwrap_or(0);
        self.output_tokens += usage.output_tokens.unwrap_or(0);
        self.cache_creation_tokens += usage.cache_creation_tokens.unwrap_or(0);
        self.cache_read_tokens += usage.cache_read_tokens.unwrap_or(0);
        if let Some(value) = usage.estimated_usd {
            self.estimated_usd = Some(self.estimated_usd.unwrap_or(0.0) + value);
        }
    }

    fn to_response(&self) -> UsageTotalsResponse {
        UsageTotalsResponse {
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_creation_tokens: self.cache_creation_tokens,
            cache_read_tokens: self.cache_read_tokens,
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
}

fn run_has_attribution(run: &AgentRun) -> bool {
    run.harness.is_some()
        || run.provider_session_id.is_some()
        || run.upstream_provider.is_some()
        || run.provider_profile.is_some()
        || run.effective_model_id.is_some()
        || run.effective_effort.is_some()
}

fn sum_message_usage(messages: &[&ChatMessage]) -> UsageTotalsResponse {
    sum_usage_samples(messages.iter().map(|message| UsageSample {
        conversation_id: message.conversation_id.map(|id| id.as_str()),
        harness: message.provider_harness,
        provider_session_id: message.provider_session_id.clone(),
        occurred_at: message.created_at,
        usage: AgentRunUsage {
            input_tokens: message.input_tokens,
            output_tokens: message.output_tokens,
            cache_creation_tokens: message.cache_creation_tokens,
            cache_read_tokens: message.cache_read_tokens,
            estimated_usd: message.estimated_usd,
        },
    }))
}

fn sum_run_usage(runs: &[&AgentRun]) -> UsageTotalsResponse {
    sum_usage_samples(runs.iter().map(|run| UsageSample {
        conversation_id: Some(run.conversation_id.as_str()),
        harness: run.harness,
        provider_session_id: run.provider_session_id.clone(),
        occurred_at: run.started_at,
        usage: AgentRunUsage {
            input_tokens: run.input_tokens,
            output_tokens: run.output_tokens,
            cache_creation_tokens: run.cache_creation_tokens,
            cache_read_tokens: run.cache_read_tokens,
            estimated_usd: run.estimated_usd,
        },
    }))
}

fn sum_usage_samples(samples: impl IntoIterator<Item = UsageSample>) -> UsageTotalsResponse {
    let mut total = UsageAccumulator::default();
    let mut codex_groups: HashMap<UsageSeriesKey, Vec<UsageSample>> = HashMap::new();

    for sample in samples {
        if sample.harness == Some(AgentHarnessKind::Codex) && sample.provider_session_id.is_some() {
            codex_groups
                .entry(UsageSeriesKey {
                    conversation_id: sample.conversation_id.clone(),
                    provider_session_id: sample.provider_session_id.clone(),
                })
                .or_default()
                .push(sample);
        } else {
            total.add_usage(&sample.usage);
        }
    }

    for mut samples in codex_groups.into_values() {
        samples.sort_by_key(|sample| sample.occurred_at);
        total.add_usage(&normalize_codex_stats_usage_series(&samples));
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

    AgentRunUsage {
        input_tokens: normalize_token_series(
            &samples
                .iter()
                .map(|sample| sample.usage.input_tokens)
                .collect::<Vec<_>>(),
            cumulative,
        ),
        output_tokens: normalize_token_series(
            &samples
                .iter()
                .map(|sample| sample.usage.output_tokens)
                .collect::<Vec<_>>(),
            cumulative,
        ),
        cache_creation_tokens: normalize_token_series(
            &samples
                .iter()
                .map(|sample| sample.usage.cache_creation_tokens)
                .collect::<Vec<_>>(),
            cumulative,
        ),
        cache_read_tokens: normalize_token_series(
            &samples
                .iter()
                .map(|sample| sample.usage.cache_read_tokens)
                .collect::<Vec<_>>(),
            cumulative,
        ),
        estimated_usd: normalize_cost_series(
            &samples
                .iter()
                .map(|sample| sample.usage.estimated_usd)
                .collect::<Vec<_>>(),
            cumulative,
        ),
    }
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
    let raw_sum = values.iter().copied().sum::<u64>();
    let large_enough = last >= 1_000_000 || raw_sum >= 10_000_000;
    large_enough && raw_sum >= last.saturating_mul(2)
}

fn normalize_token_series(values: &[Option<u64>], cumulative: bool) -> Option<u64> {
    let values: Vec<u64> = values.iter().copied().flatten().collect();
    if values.is_empty() {
        return None;
    }
    if cumulative && values.windows(2).all(|window| window[1] >= window[0]) {
        return values.last().copied();
    }
    Some(values.iter().copied().sum())
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

fn aggregate_message_buckets(
    messages: &[&ChatMessage],
    key_fn: impl Fn(&ChatMessage) -> Option<String>,
) -> Vec<UsageBucketResponse> {
    let mut buckets: BTreeMap<String, (u64, Vec<&ChatMessage>)> = BTreeMap::new();
    for message in messages {
        let key = key_fn(message).unwrap_or_else(|| "unknown".to_string());
        let entry = buckets.entry(key).or_insert_with(|| (0, Vec::new()));
        entry.0 += 1;
        entry.1.push(*message);
    }
    buckets
        .into_iter()
        .map(|(key, (count, messages))| UsageBucketResponse {
            key,
            count,
            usage: sum_message_usage(&messages),
        })
        .collect()
}

fn aggregate_run_buckets(
    runs: &[&AgentRun],
    key_fn: impl Fn(&AgentRun) -> Option<String>,
) -> Vec<UsageBucketResponse> {
    let mut buckets: BTreeMap<String, (u64, Vec<&AgentRun>)> = BTreeMap::new();
    for run in runs {
        let key = key_fn(run).unwrap_or_else(|| "unknown".to_string());
        let entry = buckets.entry(key).or_insert_with(|| (0, Vec::new()));
        entry.0 += 1;
        entry.1.push(*run);
    }
    buckets
        .into_iter()
        .map(|(key, (count, runs))| UsageBucketResponse {
            key,
            count,
            usage: sum_run_usage(&runs),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    use crate::domain::agents::ProviderSessionRef;
    use crate::domain::entities::IdeationSessionId;

    #[test]
    fn codex_cumulative_message_usage_uses_latest_sample_across_totals_and_buckets() {
        let session_id = IdeationSessionId::new();
        let mut conversation = ChatConversation::new_ideation(session_id.clone());
        conversation.set_provider_session_ref(ProviderSessionRef {
            harness: AgentHarnessKind::Codex,
            provider_session_id: "thread-cumulative".to_string(),
        });
        let messages = codex_messages(
            &conversation,
            &session_id,
            "thread-cumulative",
            &[1_000_000, 2_000_000, 3_000_000],
        );

        let response = build_conversation_stats_response(&conversation, &messages, &[]);

        assert_eq!(response.usage_coverage.effective_totals_source, "messages");
        assert_eq!(response.message_usage_totals.input_tokens, 3_000_000);
        assert_eq!(response.message_usage_totals.output_tokens, 30_000);
        assert_eq!(response.message_usage_totals.cache_read_tokens, 2_999_000);
        assert_eq!(response.effective_usage_totals.input_tokens, 3_000_000);
        assert_eq!(response.by_harness[0].usage.input_tokens, 3_000_000);
        assert_eq!(response.by_model[0].usage.input_tokens, 3_000_000);
        assert_eq!(response.by_effort[0].usage.input_tokens, 3_000_000);
    }

    #[test]
    fn codex_small_message_usage_keeps_per_turn_sum() {
        let session_id = IdeationSessionId::new();
        let conversation = ChatConversation::new_ideation(session_id.clone());
        let messages = codex_messages(&conversation, &session_id, "thread-small", &[100, 200, 300]);

        let response = build_conversation_stats_response(&conversation, &messages, &[]);

        assert_eq!(response.message_usage_totals.input_tokens, 600);
        assert_eq!(response.message_usage_totals.output_tokens, 6);
        assert_eq!(response.message_usage_totals.cache_read_tokens, 0);
    }

    #[test]
    fn codex_non_monotonic_message_usage_keeps_per_turn_sum() {
        let session_id = IdeationSessionId::new();
        let conversation = ChatConversation::new_ideation(session_id.clone());
        let messages = codex_messages(
            &conversation,
            &session_id,
            "thread-nonmonotonic",
            &[2_000_000, 1_000_000, 3_000_000],
        );

        let response = build_conversation_stats_response(&conversation, &messages, &[]);

        assert_eq!(response.message_usage_totals.input_tokens, 6_000_000);
        assert_eq!(response.message_usage_totals.output_tokens, 60_000);
    }

    #[test]
    fn codex_cumulative_run_usage_uses_latest_sample_when_messages_lack_usage() {
        let session_id = IdeationSessionId::new();
        let conversation = ChatConversation::new_ideation(session_id);
        let runs = codex_runs(
            &conversation,
            "thread-cumulative",
            &[1_000_000, 2_000_000, 3_000_000],
        );

        let response = build_conversation_stats_response(&conversation, &[], &runs);

        assert_eq!(response.usage_coverage.effective_totals_source, "runs");
        assert_eq!(response.run_usage_totals.input_tokens, 3_000_000);
        assert_eq!(response.run_usage_totals.output_tokens, 30_000);
        assert_eq!(response.effective_usage_totals.input_tokens, 3_000_000);
        assert_eq!(response.by_harness[0].usage.input_tokens, 3_000_000);
    }

    #[test]
    fn usage_series_helpers_handle_empty_small_cumulative_and_non_monotonic_values() {
        assert!(!looks_like_cumulative_token_series(&[]));
        assert!(!looks_like_cumulative_token_series(&[Some(10), Some(20)]));
        assert!(!looks_like_cumulative_token_series(&[
            Some(2_000_000),
            Some(1_000_000),
            Some(3_000_000),
        ]));
        assert!(looks_like_cumulative_token_series(&[
            Some(1_000_000),
            Some(2_000_000),
            Some(3_000_000),
        ]));

        assert_eq!(normalize_token_series(&[], true), None);
        assert_eq!(
            normalize_token_series(&[Some(10), Some(20)], false),
            Some(30)
        );
        assert_eq!(
            normalize_token_series(&[Some(1_000_000), Some(2_000_000)], true),
            Some(2_000_000)
        );
        assert_eq!(normalize_cost_series(&[], true), None);
        assert_eq!(
            normalize_cost_series(&[Some(1.0), Some(2.0)], true),
            Some(2.0)
        );
        assert_eq!(
            normalize_cost_series(&[Some(2.0), Some(1.0)], true),
            Some(3.0)
        );
    }

    fn codex_messages(
        conversation: &ChatConversation,
        session_id: &IdeationSessionId,
        provider_session_id: &str,
        input_tokens: &[u64],
    ) -> Vec<ChatMessage> {
        let now = Utc::now();
        input_tokens
            .iter()
            .copied()
            .enumerate()
            .map(|(index, tokens)| {
                let mut message = ChatMessage::orchestrator_in_session(session_id.clone(), "done");
                message.conversation_id = Some(conversation.id);
                message.provider_harness = Some(AgentHarnessKind::Codex);
                message.provider_session_id = Some(provider_session_id.to_string());
                message.upstream_provider = Some("openai".to_string());
                message.effective_model_id = Some("gpt-5.5".to_string());
                message.effective_effort = Some("xhigh".to_string());
                message.input_tokens = Some(tokens);
                message.output_tokens = Some(tokens / 100);
                message.cache_read_tokens = tokens.checked_sub(1_000);
                message.created_at = now + Duration::seconds(index as i64);
                message
            })
            .collect()
    }

    fn codex_runs(
        conversation: &ChatConversation,
        provider_session_id: &str,
        input_tokens: &[u64],
    ) -> Vec<AgentRun> {
        let now = Utc::now();
        input_tokens
            .iter()
            .copied()
            .enumerate()
            .map(|(index, tokens)| {
                let mut run = AgentRun::new(conversation.id);
                run.harness = Some(AgentHarnessKind::Codex);
                run.provider_session_id = Some(provider_session_id.to_string());
                run.upstream_provider = Some("openai".to_string());
                run.effective_model_id = Some("gpt-5.5".to_string());
                run.effective_effort = Some("xhigh".to_string());
                run.input_tokens = Some(tokens);
                run.output_tokens = Some(tokens / 100);
                run.cache_read_tokens = tokens.checked_sub(1_000);
                run.started_at = now + Duration::seconds(index as i64);
                run
            })
            .collect()
    }
}
