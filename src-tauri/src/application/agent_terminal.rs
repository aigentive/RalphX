use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, Mutex};

use crate::application::agent_conversation_workspace::resolve_valid_agent_conversation_workspace_path;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceStatus, ChatContextType,
    ChatConversationId, Project, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, ChatConversationRepository, ProjectRepository,
};
use crate::error::{AppError, AppResult};

pub const AGENT_TERMINAL_EVENT: &str = "agent_terminal:event";

const DEFAULT_TERMINAL_ID: &str = "default";
const MAX_TERMINAL_ID_LEN: usize = 64;
const MIN_TERMINAL_COLS: u16 = 2;
const MAX_TERMINAL_COLS: u16 = 500;
const MIN_TERMINAL_ROWS: u16 = 2;
const MAX_TERMINAL_ROWS: u16 = 200;
const MAX_WRITE_BYTES: usize = 64 * 1024;
const MAX_HISTORY_BYTES: usize = 200 * 1024;

#[derive(Debug, Clone)]
pub struct AgentTerminalOpenRequest {
    pub conversation_id: ChatConversationId,
    pub terminal_id: String,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone)]
pub struct AgentTerminalWriteRequest {
    pub conversation_id: ChatConversationId,
    pub terminal_id: String,
    pub data: String,
}

#[derive(Debug, Clone)]
pub struct AgentTerminalResizeRequest {
    pub conversation_id: ChatConversationId,
    pub terminal_id: String,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone)]
pub struct AgentTerminalCloseRequest {
    pub conversation_id: ChatConversationId,
    pub terminal_id: String,
    pub delete_history: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentTerminalStatus {
    Running,
    Exited,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTerminalSnapshot {
    pub conversation_id: String,
    pub terminal_id: String,
    pub cwd: String,
    pub workspace_branch: String,
    pub status: AgentTerminalStatus,
    pub pid: Option<u32>,
    pub history: String,
    pub exit_code: Option<i32>,
    pub exit_signal: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTerminalEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub conversation_id: String,
    pub terminal_id: String,
    pub cwd: Option<String>,
    pub workspace_branch: Option<String>,
    pub data: Option<String>,
    pub message: Option<String>,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub exit_signal: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct PtySpawnRequest {
    pub cwd: PathBuf,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Clone)]
pub struct AgentTerminalEventSink {
    tx: mpsc::UnboundedSender<AgentTerminalBackendEvent>,
}

impl AgentTerminalEventSink {
    pub fn output(&self, data: String) {
        let _ = self.tx.send(AgentTerminalBackendEvent::Output(data));
    }

    pub fn exited(&self, exit_code: Option<i32>, exit_signal: Option<String>) {
        let _ = self.tx.send(AgentTerminalBackendEvent::Exited {
            exit_code,
            exit_signal,
        });
    }

    pub fn error(&self, message: String) {
        let _ = self.tx.send(AgentTerminalBackendEvent::Error(message));
    }
}

pub trait AgentTerminalProcess: Send + Sync {
    fn pid(&self) -> Option<u32>;
    fn write(&self, data: &[u8]) -> AppResult<()>;
    fn resize(&self, cols: u16, rows: u16) -> AppResult<()>;
    fn kill(&self) -> AppResult<()>;
}

pub trait AgentTerminalProcessFactory: Send + Sync {
    fn spawn(
        &self,
        request: PtySpawnRequest,
        sink: AgentTerminalEventSink,
    ) -> AppResult<Arc<dyn AgentTerminalProcess>>;
}

#[derive(Clone)]
pub struct AgentTerminalService {
    sessions: Arc<Mutex<HashMap<TerminalKey, TerminalSession>>>,
    process_factory: Arc<dyn AgentTerminalProcessFactory>,
}

impl AgentTerminalService {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            process_factory: Arc::new(PortablePtyProcessFactory),
        }
    }

    #[cfg(test)]
    pub fn with_process_factory(process_factory: Arc<dyn AgentTerminalProcessFactory>) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            process_factory,
        }
    }

    pub async fn open(
        &self,
        request: AgentTerminalOpenRequest,
        deps: AgentTerminalWorkspaceDeps<'_>,
        app_handle: Option<AppHandle>,
    ) -> AppResult<AgentTerminalSnapshot> {
        validate_terminal_id(&request.terminal_id)?;
        validate_dimensions(request.cols, request.rows)?;
        let launch = resolve_terminal_launch(&request.conversation_id, deps).await?;
        self.open_resolved(request, launch, app_handle, "started")
            .await
    }

    pub async fn write(&self, request: AgentTerminalWriteRequest) -> AppResult<()> {
        validate_terminal_id(&request.terminal_id)?;
        if request.data.len() > MAX_WRITE_BYTES {
            return Err(AppError::Validation(format!(
                "Terminal write payload exceeds {} bytes",
                MAX_WRITE_BYTES
            )));
        }

        let key = TerminalKey::new(&request.conversation_id, &request.terminal_id);
        let process = {
            let sessions = self.sessions.lock().await;
            let session = sessions
                .get(&key)
                .ok_or_else(|| AppError::NotFound("Agent terminal is not open".to_string()))?;
            if session.status != AgentTerminalStatus::Running {
                return Err(AppError::Validation(
                    "Cannot write to a terminal that is not running".to_string(),
                ));
            }
            Arc::clone(&session.process)
        };
        process.write(request.data.as_bytes())
    }

    pub async fn resize(
        &self,
        request: AgentTerminalResizeRequest,
    ) -> AppResult<AgentTerminalSnapshot> {
        validate_terminal_id(&request.terminal_id)?;
        validate_dimensions(request.cols, request.rows)?;
        let key = TerminalKey::new(&request.conversation_id, &request.terminal_id);
        let process = {
            let mut sessions = self.sessions.lock().await;
            let session = sessions
                .get_mut(&key)
                .ok_or_else(|| AppError::NotFound("Agent terminal is not open".to_string()))?;
            if !terminal_dimensions_changed(session, request.cols, request.rows) {
                return Ok(session.snapshot());
            }
            session.cols = request.cols;
            session.rows = request.rows;
            session.updated_at = Utc::now();
            Arc::clone(&session.process)
        };
        process.resize(request.cols, request.rows)?;
        self.snapshot(&key).await
    }

    pub async fn clear(
        &self,
        conversation_id: ChatConversationId,
        terminal_id: String,
        app_handle: Option<AppHandle>,
    ) -> AppResult<AgentTerminalSnapshot> {
        validate_terminal_id(&terminal_id)?;
        let key = TerminalKey::new(&conversation_id, &terminal_id);
        let event = {
            let mut sessions = self.sessions.lock().await;
            let session = sessions
                .get_mut(&key)
                .ok_or_else(|| AppError::NotFound("Agent terminal is not open".to_string()))?;
            session.history.clear();
            session.updated_at = Utc::now();
            session.event("cleared", None, None, None, None)
        };
        emit_terminal_event(app_handle.as_ref(), &event);
        self.snapshot(&key).await
    }

    pub async fn restart(
        &self,
        request: AgentTerminalOpenRequest,
        deps: AgentTerminalWorkspaceDeps<'_>,
        app_handle: Option<AppHandle>,
    ) -> AppResult<AgentTerminalSnapshot> {
        validate_terminal_id(&request.terminal_id)?;
        validate_dimensions(request.cols, request.rows)?;
        let launch = resolve_terminal_launch(&request.conversation_id, deps).await?;
        let key = TerminalKey::new(&request.conversation_id, &request.terminal_id);
        self.close_key(&key, false).await?;
        self.open_resolved(request, launch, app_handle, "restarted")
            .await
    }

    pub async fn close(&self, request: AgentTerminalCloseRequest) -> AppResult<()> {
        validate_terminal_id(&request.terminal_id)?;
        let key = TerminalKey::new(&request.conversation_id, &request.terminal_id);
        self.close_key(&key, request.delete_history).await
    }

    pub async fn close_all(&self) {
        let keys = {
            let sessions = self.sessions.lock().await;
            sessions.keys().cloned().collect::<Vec<_>>()
        };
        for key in keys {
            let _ = self.close_key(&key, true).await;
        }
    }

    async fn open_resolved(
        &self,
        request: AgentTerminalOpenRequest,
        launch: TerminalLaunch,
        app_handle: Option<AppHandle>,
        start_event_type: &str,
    ) -> AppResult<AgentTerminalSnapshot> {
        let key = TerminalKey::new(&request.conversation_id, &request.terminal_id);
        let terminal_id = normalize_terminal_id(&request.terminal_id);
        if let Some(snapshot) = self
            .existing_snapshot_after_resize(&key, request.cols, request.rows)
            .await?
        {
            return Ok(snapshot);
        }

        let (tx, rx) = mpsc::unbounded_channel();
        let sink = AgentTerminalEventSink { tx };
        let process = self.process_factory.spawn(
            PtySpawnRequest {
                cwd: launch.cwd.clone(),
                cols: request.cols,
                rows: request.rows,
            },
            sink,
        )?;
        let now = Utc::now();
        let session = TerminalSession {
            conversation_id: request.conversation_id.as_str().to_string(),
            terminal_id,
            cwd: launch.cwd.to_string_lossy().to_string(),
            workspace_branch: launch.workspace.branch_name,
            status: AgentTerminalStatus::Running,
            pid: process.pid(),
            process,
            history: String::new(),
            exit_code: None,
            exit_signal: None,
            cols: request.cols,
            rows: request.rows,
            updated_at: now,
        };
        let event = session.event(start_event_type, None, None, None, None);
        {
            let mut sessions = self.sessions.lock().await;
            sessions.insert(key.clone(), session);
        }
        emit_terminal_event(app_handle.as_ref(), &event);
        self.spawn_event_pump(key.clone(), rx, app_handle);
        self.snapshot(&key).await
    }

    async fn existing_snapshot_after_resize(
        &self,
        key: &TerminalKey,
        cols: u16,
        rows: u16,
    ) -> AppResult<Option<AgentTerminalSnapshot>> {
        let process = {
            let mut sessions = self.sessions.lock().await;
            let Some(session) = sessions.get_mut(key) else {
                return Ok(None);
            };
            if session.status == AgentTerminalStatus::Running {
                if !terminal_dimensions_changed(session, cols, rows) {
                    return Ok(Some(session.snapshot()));
                }
                session.cols = cols;
                session.rows = rows;
                session.updated_at = Utc::now();
                Some(Arc::clone(&session.process))
            } else {
                None
            }
        };
        if let Some(process) = process {
            process.resize(cols, rows)?;
        }
        Ok(Some(self.snapshot(key).await?))
    }

    async fn snapshot(&self, key: &TerminalKey) -> AppResult<AgentTerminalSnapshot> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(key)
            .ok_or_else(|| AppError::NotFound("Agent terminal is not open".to_string()))?;
        Ok(session.snapshot())
    }

    async fn close_key(&self, key: &TerminalKey, _delete_history: bool) -> AppResult<()> {
        let session = {
            let mut sessions = self.sessions.lock().await;
            sessions.remove(key)
        };
        if let Some(session) = session {
            session.process.kill()?;
        }
        Ok(())
    }

    fn spawn_event_pump(
        &self,
        key: TerminalKey,
        mut rx: mpsc::UnboundedReceiver<AgentTerminalBackendEvent>,
        app_handle: Option<AppHandle>,
    ) {
        let sessions = Arc::clone(&self.sessions);
        tauri::async_runtime::spawn(async move {
            while let Some(event) = rx.recv().await {
                let frontend_event = {
                    let mut sessions = sessions.lock().await;
                    let Some(session) = sessions.get_mut(&key) else {
                        continue;
                    };
                    match event {
                        AgentTerminalBackendEvent::Output(data) => {
                            append_capped_history(&mut session.history, &data);
                            session.updated_at = Utc::now();
                            session.event("output", Some(data), None, None, None)
                        }
                        AgentTerminalBackendEvent::Exited {
                            exit_code,
                            exit_signal,
                        } => {
                            session.status = AgentTerminalStatus::Exited;
                            session.exit_code = exit_code;
                            session.exit_signal = exit_signal.clone();
                            session.updated_at = Utc::now();
                            session.event("exited", None, None, exit_code, exit_signal)
                        }
                        AgentTerminalBackendEvent::Error(message) => {
                            session.status = AgentTerminalStatus::Error;
                            session.updated_at = Utc::now();
                            session.event("error", None, Some(message), None, None)
                        }
                    }
                };
                emit_terminal_event(app_handle.as_ref(), &frontend_event);
            }
        });
    }
}

impl Default for AgentTerminalService {
    fn default() -> Self {
        Self::new()
    }
}

pub struct AgentTerminalWorkspaceDeps<'a> {
    pub chat_conversation_repo: &'a Arc<dyn ChatConversationRepository>,
    pub workspace_repo: &'a Arc<dyn AgentConversationWorkspaceRepository>,
    pub project_repo: &'a Arc<dyn ProjectRepository>,
}

#[derive(Debug, Clone)]
struct TerminalLaunch {
    cwd: PathBuf,
    workspace: AgentConversationWorkspace,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TerminalKey {
    conversation_id: String,
    terminal_id: String,
}

impl TerminalKey {
    fn new(conversation_id: &ChatConversationId, terminal_id: &str) -> Self {
        Self {
            conversation_id: conversation_id.as_str().to_string(),
            terminal_id: normalize_terminal_id(terminal_id),
        }
    }
}

struct TerminalSession {
    conversation_id: String,
    terminal_id: String,
    cwd: String,
    workspace_branch: String,
    status: AgentTerminalStatus,
    pid: Option<u32>,
    process: Arc<dyn AgentTerminalProcess>,
    history: String,
    exit_code: Option<i32>,
    exit_signal: Option<String>,
    cols: u16,
    rows: u16,
    updated_at: DateTime<Utc>,
}

impl TerminalSession {
    fn snapshot(&self) -> AgentTerminalSnapshot {
        AgentTerminalSnapshot {
            conversation_id: self.conversation_id.clone(),
            terminal_id: self.terminal_id.clone(),
            cwd: self.cwd.clone(),
            workspace_branch: self.workspace_branch.clone(),
            status: self.status.clone(),
            pid: self.pid,
            history: self.history.clone(),
            exit_code: self.exit_code,
            exit_signal: self.exit_signal.clone(),
            updated_at: self.updated_at,
        }
    }

    fn event(
        &self,
        event_type: &str,
        data: Option<String>,
        message: Option<String>,
        exit_code: Option<i32>,
        exit_signal: Option<String>,
    ) -> AgentTerminalEvent {
        AgentTerminalEvent {
            event_type: event_type.to_string(),
            conversation_id: self.conversation_id.clone(),
            terminal_id: self.terminal_id.clone(),
            cwd: matches!(event_type, "started" | "restarted").then(|| self.cwd.clone()),
            workspace_branch: matches!(event_type, "started" | "restarted")
                .then(|| self.workspace_branch.clone()),
            data,
            message,
            pid: self.pid,
            exit_code,
            exit_signal,
            updated_at: self.updated_at,
        }
    }
}

enum AgentTerminalBackendEvent {
    Output(String),
    Exited {
        exit_code: Option<i32>,
        exit_signal: Option<String>,
    },
    Error(String),
}

struct PortablePtyProcessFactory;

impl AgentTerminalProcessFactory for PortablePtyProcessFactory {
    fn spawn(
        &self,
        request: PtySpawnRequest,
        sink: AgentTerminalEventSink,
    ) -> AppResult<Arc<dyn AgentTerminalProcess>> {
        // The cwd has already been resolved by the workspace validator. Validate at the
        // process launch sink as well so path safety remains local to the sink.
        if !request.cwd.is_absolute() || !request.cwd.is_dir() {
            return Err(AppError::Validation(format!(
                "Terminal cwd is not an existing absolute directory: {}",
                request.cwd.display()
            )));
        }

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: request.rows,
                cols: request.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| AppError::Infrastructure(format!("Failed to open PTY: {error}")))?;

        let command = build_terminal_command(&request.cwd);
        let child = pair.slave.spawn_command(command).map_err(|error| {
            AppError::Infrastructure(format!("Failed to spawn terminal shell: {error}"))
        })?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(|error| {
            AppError::Infrastructure(format!("Failed to open terminal reader: {error}"))
        })?;
        let writer = pair.master.take_writer().map_err(|error| {
            AppError::Infrastructure(format!("Failed to open terminal writer: {error}"))
        })?;
        let pid = child.process_id();
        let process = Arc::new(PortablePtyProcess {
            child: std::sync::Mutex::new(child),
            master: std::sync::Mutex::new(pair.master),
            writer: std::sync::Mutex::new(writer),
            pid,
        });

        std::thread::spawn(move || {
            let mut buffer = [0_u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        sink.exited(None, None);
                        break;
                    }
                    Ok(n) => {
                        sink.output(String::from_utf8_lossy(&buffer[..n]).to_string());
                    }
                    Err(error) => {
                        sink.error(format!("Terminal read failed: {error}"));
                        break;
                    }
                }
            }
        });

        Ok(process)
    }
}

struct PortablePtyProcess {
    child: std::sync::Mutex<Box<dyn portable_pty::Child + Send>>,
    master: std::sync::Mutex<Box<dyn MasterPty + Send>>,
    writer: std::sync::Mutex<Box<dyn Write + Send>>,
    pid: Option<u32>,
}

impl AgentTerminalProcess for PortablePtyProcess {
    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn write(&self, data: &[u8]) -> AppResult<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| AppError::Infrastructure("Terminal writer lock poisoned".to_string()))?;
        writer
            .write_all(data)
            .map_err(|error| AppError::Infrastructure(format!("Terminal write failed: {error}")))?;
        writer
            .flush()
            .map_err(|error| AppError::Infrastructure(format!("Terminal flush failed: {error}")))
    }

    fn resize(&self, cols: u16, rows: u16) -> AppResult<()> {
        let master = self
            .master
            .lock()
            .map_err(|_| AppError::Infrastructure("Terminal PTY lock poisoned".to_string()))?;
        master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| AppError::Infrastructure(format!("Terminal resize failed: {error}")))
    }

    fn kill(&self) -> AppResult<()> {
        let mut child = self
            .child
            .lock()
            .map_err(|_| AppError::Infrastructure("Terminal process lock poisoned".to_string()))?;
        child
            .kill()
            .map_err(|error| AppError::Infrastructure(format!("Terminal kill failed: {error}")))
    }
}

async fn resolve_terminal_launch(
    conversation_id: &ChatConversationId,
    deps: AgentTerminalWorkspaceDeps<'_>,
) -> AppResult<TerminalLaunch> {
    let conversation = deps
        .chat_conversation_repo
        .get_by_id(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!("Conversation not found: {}", conversation_id))
        })?;
    if conversation.context_type != ChatContextType::Project {
        return Err(AppError::Validation(
            "Terminals are only available for project agent conversations".to_string(),
        ));
    }

    let workspace = deps
        .workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::Validation(
                "Terminal unavailable for branchless chat conversations".to_string(),
            )
        })?;
    if workspace.status == AgentConversationWorkspaceStatus::Missing {
        return Err(AppError::Validation(
            "Terminal unavailable because the conversation workspace is missing".to_string(),
        ));
    }
    if workspace.linked_ideation_session_id.is_some() || workspace.linked_plan_branch_id.is_some() {
        return Err(AppError::Validation(
            "Terminal unavailable because ideation or execution owns this workspace".to_string(),
        ));
    }

    let project_id = ProjectId::from_string(conversation.context_id.clone());
    let project = deps
        .project_repo
        .get_by_id(&project_id)
        .await?
        .ok_or_else(|| AppError::ProjectNotFound(conversation.context_id.clone()))?;
    validate_workspace_project(&project, &workspace)?;
    let cwd = resolve_valid_agent_conversation_workspace_path(&project, &workspace).await?;
    Ok(TerminalLaunch { cwd, workspace })
}

fn validate_workspace_project(
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> AppResult<()> {
    if workspace.project_id != project.id {
        return Err(AppError::Validation(
            "Terminal workspace does not belong to the conversation project".to_string(),
        ));
    }
    Ok(())
}

fn normalize_terminal_id(terminal_id: &str) -> String {
    let trimmed = terminal_id.trim();
    if trimmed.is_empty() {
        DEFAULT_TERMINAL_ID.to_string()
    } else {
        trimmed.to_string()
    }
}

fn validate_terminal_id(terminal_id: &str) -> AppResult<()> {
    let terminal_id = normalize_terminal_id(terminal_id);
    if terminal_id.len() > MAX_TERMINAL_ID_LEN {
        return Err(AppError::Validation(format!(
            "Terminal id exceeds {} characters",
            MAX_TERMINAL_ID_LEN
        )));
    }
    if !terminal_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(AppError::Validation(
            "Terminal id may only contain letters, numbers, '.', '-', and '_'".to_string(),
        ));
    }
    Ok(())
}

fn validate_dimensions(cols: u16, rows: u16) -> AppResult<()> {
    if !(MIN_TERMINAL_COLS..=MAX_TERMINAL_COLS).contains(&cols) {
        return Err(AppError::Validation(format!(
            "Terminal columns must be between {} and {}",
            MIN_TERMINAL_COLS, MAX_TERMINAL_COLS
        )));
    }
    if !(MIN_TERMINAL_ROWS..=MAX_TERMINAL_ROWS).contains(&rows) {
        return Err(AppError::Validation(format!(
            "Terminal rows must be between {} and {}",
            MIN_TERMINAL_ROWS, MAX_TERMINAL_ROWS
        )));
    }
    Ok(())
}

fn append_capped_history(history: &mut String, data: &str) {
    history.push_str(data);
    if history.len() <= MAX_HISTORY_BYTES {
        return;
    }
    let mut start = history.len() - MAX_HISTORY_BYTES;
    while !history.is_char_boundary(start) {
        start += 1;
    }
    history.drain(..start);
}

fn terminal_dimensions_changed(session: &TerminalSession, cols: u16, rows: u16) -> bool {
    session.cols != cols || session.rows != rows
}

fn emit_terminal_event(app_handle: Option<&AppHandle>, event: &AgentTerminalEvent) {
    if let Some(app_handle) = app_handle {
        let _ = app_handle.emit(AGENT_TERMINAL_EVENT, event);
    }
}

fn terminal_shell_path() -> &'static str {
    if cfg!(target_os = "macos") && std::path::Path::new("/bin/zsh").is_file() {
        "/bin/zsh"
    } else {
        "/bin/sh"
    }
}

fn build_terminal_command(cwd: &Path) -> CommandBuilder {
    let shell = terminal_shell_path();
    let mut command = CommandBuilder::new(shell);
    if cfg!(target_os = "macos") && shell == "/bin/zsh" {
        command.arg("-l");
    }
    command.cwd(cwd);
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("CLICOLOR", "1");
    command.env("SHELL", shell);
    command.env("PWD", cwd.as_os_str());
    command.env("PATH", terminal_env_path());
    command
}

fn terminal_env_path() -> std::ffi::OsString {
    crate::infrastructure::tool_paths::agent_subprocess_env_path()
}

#[cfg(test)]
fn terminal_env_path_from_parts(
    existing_path: Option<&std::ffi::OsStr>,
    home_dir: Option<&Path>,
) -> std::ffi::OsString {
    crate::infrastructure::tool_paths::agent_subprocess_env_path_from_parts(existing_path, home_dir)
}

#[cfg(test)]
pub(crate) fn build_terminal_command_for_test(cwd: &Path) -> CommandBuilder {
    build_terminal_command(cwd)
}

#[cfg(test)]
pub(crate) fn terminal_env_path_from_parts_for_test(
    existing_path: Option<&std::ffi::OsStr>,
    home_dir: Option<&Path>,
) -> std::ffi::OsString {
    terminal_env_path_from_parts(existing_path, home_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_id_rejects_path_like_components() {
        assert!(validate_terminal_id("default").is_ok());
        assert_eq!(normalize_terminal_id(" default "), "default");
        assert_eq!(normalize_terminal_id(""), DEFAULT_TERMINAL_ID);
        assert!(validate_terminal_id("../main").is_err());
        assert!(validate_terminal_id("nested/path").is_err());
        assert!(validate_terminal_id("term 1").is_err());
    }

    #[test]
    fn dimensions_are_bounded() {
        assert!(validate_dimensions(80, 24).is_ok());
        assert!(validate_dimensions(1, 24).is_err());
        assert!(validate_dimensions(80, 1).is_err());
        assert!(validate_dimensions(MAX_TERMINAL_COLS + 1, 24).is_err());
        assert!(validate_dimensions(80, MAX_TERMINAL_ROWS + 1).is_err());
    }

    #[test]
    fn history_cap_keeps_valid_utf8_suffix() {
        let mut history = "α".repeat(MAX_HISTORY_BYTES / 2);
        append_capped_history(&mut history, &"β".repeat(MAX_HISTORY_BYTES));
        assert!(history.len() <= MAX_HISTORY_BYTES);
        assert!(history.is_char_boundary(0));
        assert!(history.ends_with('β'));
    }

    #[test]
    fn terminal_dimension_change_detection_skips_identical_sizes() {
        let session = TerminalSession {
            conversation_id: "conversation-1".to_string(),
            terminal_id: DEFAULT_TERMINAL_ID.to_string(),
            cwd: "/tmp/project".to_string(),
            workspace_branch: "feature/agent".to_string(),
            status: AgentTerminalStatus::Running,
            pid: Some(42),
            process: Arc::new(NoopTerminalProcess),
            history: String::new(),
            exit_code: None,
            exit_signal: None,
            cols: 120,
            rows: 32,
            updated_at: Utc::now(),
        };

        assert!(!terminal_dimensions_changed(&session, 120, 32));
        assert!(terminal_dimensions_changed(&session, 121, 32));
        assert!(terminal_dimensions_changed(&session, 120, 33));
    }

    struct NoopTerminalProcess;

    impl AgentTerminalProcess for NoopTerminalProcess {
        fn pid(&self) -> Option<u32> {
            Some(42)
        }

        fn write(&self, _data: &[u8]) -> AppResult<()> {
            Ok(())
        }

        fn resize(&self, _cols: u16, _rows: u16) -> AppResult<()> {
            Ok(())
        }

        fn kill(&self) -> AppResult<()> {
            Ok(())
        }
    }
}
