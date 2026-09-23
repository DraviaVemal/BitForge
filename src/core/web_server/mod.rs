use std::collections::VecDeque;
use std::convert::Infallible;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper::service::service_fn;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, watch};

mod api;
mod bitbake_build;
mod metadata;
mod routes;

use routes::handle;
pub(crate) use routes::{json, text};

pub(crate) type ResponseBody = BoxBody<Bytes, Infallible>;

const BITBAKE_ACTIVITY_KINDS: [&str; 4] = ["metadata", "deptree", "plan", "layout"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerMessage {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

impl ServerMessage {
    pub fn new(kind: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            kind: kind.into(),
            payload: Some(payload),
        }
    }
}

#[derive(Default)]
struct PollBuffer {
    next_id: u64,
    items: VecDeque<(u64, ServerMessage)>,
}

impl PollBuffer {
    const CAPACITY: usize = 512;

    fn push(&mut self, message: ServerMessage) -> u64 {
        let message_id = self.next_id;
        self.next_id += 1;
        self.items.push_back((message_id, message));
        while self.items.len() > Self::CAPACITY {
            self.items.pop_front();
        }
        message_id
    }

    fn since(&self, cursor: u64) -> (u64, Vec<ServerMessage>) {
        let messages: Vec<ServerMessage> = self
            .items
            .iter()
            .filter(|(message_id, _)| *message_id >= cursor)
            .map(|(_, message)| message.clone())
            .collect();
        (self.next_id, messages)
    }
}

struct AppState {
    working_directory: PathBuf,
    outbound: broadcast::Sender<ServerMessage>,
    poll_buffer: Mutex<PollBuffer>,
    build: Mutex<bitbake_build::BuildManager>,
    tasks: Mutex<TaskRegistry>,
    activity_timeout: Duration,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskEntry {
    pub id: u64,
    pub label: String,
    pub kind: String,
    pub status: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub cancellable: bool,
}

struct TaskRecord {
    entry: TaskEntry,
    key: String,
    log: VecDeque<String>,
    cancel: Option<Arc<AtomicBool>>,
}

/// Reason a task could not be started.
enum BeginError {
    /// An identical task is already running.
    Running { id: u64, label: String },
    /// The task has failed too many times and won't be retried automatically.
    Blocked { message: String },
}

#[derive(Default)]
struct TaskRegistry {
    next_id: u64,
    records: VecDeque<TaskRecord>,
    failures: HashMap<String, u32>,
}

impl TaskRegistry {
    const HISTORY: usize = 200;
    const LOG_LINES: usize = 500;
    const MAX_ATTEMPTS: u32 = 2;

    fn snapshot(&self) -> Vec<TaskEntry> {
        self.records.iter().rev().map(|record| record.entry.clone()).collect()
    }
}

impl AppState {
    fn publish(&self, message: ServerMessage) {
        self.poll_buffer.lock().unwrap().push(message.clone());
        let _ = self.outbound.send(message);
    }

    fn begin_task(&self, label: String, kind: &str, key: &str, cancel: Option<Arc<AtomicBool>>) -> u64 {
        let id;
        let started_at = unix_now();
        {
            let mut registry = self.tasks.lock().unwrap();
            registry.next_id += 1;
            id = registry.next_id;
            registry.records.push_back(TaskRecord {
                entry: TaskEntry {
                    id,
                    label: label.clone(),
                    kind: kind.to_string(),
                    status: "running".to_string(),
                    started_at,
                    finished_at: None,
                    cancellable: cancel.is_some(),
                },
                key: key.to_string(),
                log: VecDeque::new(),
                cancel,
            });
            while registry.records.len() > TaskRegistry::HISTORY {
                registry.records.pop_front();
            }
        }
        if let Err(error) = crate::core::store::record_activity_start(
            &self.working_directory,
            id as i64,
            &label,
            kind,
            "running",
            started_at,
        ) {
            debug!("failed to persist activity start: {error}");
        }
        self.publish_tasks();
        id
    }

    fn running_task_for_key(&self, key: &str) -> Option<(u64, String)> {
        let registry = self.tasks.lock().unwrap();
        registry
            .records
            .iter()
            .find(|record| record.entry.status == "running" && record.key == key)
            .map(|record| (record.entry.id, record.entry.label.clone()))
    }

    fn try_begin(
        &self,
        label: String,
        kind: &str,
        key: &str,
        cancel: Option<Arc<AtomicBool>>,
        force: bool,
    ) -> Result<u64, BeginError> {
        if force {
            // A manual retry clears the failure block for this key.
            self.tasks.lock().unwrap().failures.remove(key);
        } else if self.failed_too_often(key) {
            return Err(BeginError::Blocked {
                message: format!(
                    "'{label}' failed {} times; not retrying automatically. Use refresh to try again.",
                    TaskRegistry::MAX_ATTEMPTS
                ),
            });
        }
        if let Some((id, existing)) = self.running_task_for_key(key) {
            if !force {
                return Err(BeginError::Running { id, label: existing });
            }
            self.cancel_task(id);
        }
        Ok(self.begin_task(label, kind, key, cancel))
    }

    fn failed_too_often(&self, key: &str) -> bool {
        self.tasks
            .lock()
            .unwrap()
            .failures
            .get(key)
            .is_some_and(|count| *count >= TaskRegistry::MAX_ATTEMPTS)
    }

    fn end_task(&self, id: u64, status: &str) {
        let finished_at = unix_now();
        let mut log_text = None;
        {
            let mut registry = self.tasks.lock().unwrap();
            let mut key = None;
            if let Some(record) = registry.records.iter_mut().find(|record| record.entry.id == id) {
                record.entry.status = status.to_string();
                record.entry.finished_at = Some(finished_at);
                record.entry.cancellable = false;
                record.cancel = None;
                log_text = Some(record.log.iter().cloned().collect::<Vec<_>>().join("\n"));
                if !record.key.is_empty() {
                    key = Some(record.key.clone());
                }
            }
            if let Some(key) = key {
                match status {
                    "failed" => *registry.failures.entry(key).or_insert(0) += 1,
                    _ => {
                        registry.failures.remove(&key);
                    }
                }
            }
        }
        if let Err(error) = crate::core::store::record_activity_finish(
            &self.working_directory,
            id as i64,
            status,
            finished_at,
            log_text.as_deref(),
        ) {
            debug!("failed to persist activity finish: {error}");
        }
        self.publish_tasks();
    }

    fn append_task_log(&self, id: u64, line: &str) {
        {
            let mut registry = self.tasks.lock().unwrap();
            let Some(record) = registry.records.iter_mut().find(|record| record.entry.id == id)
            else {
                return;
            };
            record.log.push_back(line.to_string());
            while record.log.len() > TaskRegistry::LOG_LINES {
                record.log.pop_front();
            }
        }
        self.publish(ServerMessage::new(
            "task-log",
            serde_json::json!({ "id": id, "line": line }),
        ));
    }

    fn cancel_task(&self, id: u64) -> bool {
        let registry = self.tasks.lock().unwrap();
        if let Some(record) = registry.records.iter().find(|record| record.entry.id == id) {
            if let Some(flag) = &record.cancel {
                flag.store(true, Ordering::SeqCst);
                return true;
            }
        }
        false
    }

    fn cancel_all_running_tasks(&self) -> usize {
        let registry = self.tasks.lock().unwrap();
        let mut cancelled = 0;
        for record in registry.records.iter() {
            if record.entry.status == "running" {
                if let Some(flag) = &record.cancel {
                    flag.store(true, Ordering::SeqCst);
                    cancelled += 1;
                }
            }
        }
        cancelled
    }

    fn cancel_bitbake_activities(&self) -> usize {
        let registry = self.tasks.lock().unwrap();
        let mut cancelled = 0;
        for record in registry.records.iter() {
            if record.entry.status == "running"
                && BITBAKE_ACTIVITY_KINDS.contains(&record.entry.kind.as_str())
            {
                if let Some(flag) = &record.cancel {
                    flag.store(true, Ordering::SeqCst);
                    cancelled += 1;
                }
            }
        }
        cancelled
    }

    fn bitbake_activity_running(&self) -> bool {
        let registry = self.tasks.lock().unwrap();
        registry.records.iter().any(|record| {
            record.entry.status == "running"
                && BITBAKE_ACTIVITY_KINDS.contains(&record.entry.kind.as_str())
        })
    }

    fn task_detail(&self, id: u64) -> Option<(TaskEntry, Vec<String>)> {
        let registry = self.tasks.lock().unwrap();
        registry
            .records
            .iter()
            .find(|record| record.entry.id == id)
            .map(|record| (record.entry.clone(), record.log.iter().cloned().collect()))
    }

    fn tasks_snapshot(&self) -> Vec<TaskEntry> {
        self.tasks.lock().unwrap().snapshot()
    }

    fn publish_tasks(&self) {
        let snapshot = self.tasks.lock().unwrap().snapshot();
        let payload = serde_json::to_value(snapshot).unwrap_or_default();
        self.publish(ServerMessage::new("tasks", payload));
    }

    fn load_activity_history(&self) {
        if let Err(error) = crate::core::store::mark_running_cancelled(&self.working_directory, unix_now()) {
            debug!("failed to reconcile stale activities: {error}");
        }
        let records = match crate::core::store::list_activities(
            &self.working_directory,
            TaskRegistry::HISTORY as i64,
        ) {
            Ok(records) => records,
            Err(error) => {
                debug!("failed to load activity history: {error}");
                return;
            }
        };
        let mut registry = self.tasks.lock().unwrap();
        for record in records {
            let id = record.id as u64;
            if id > registry.next_id {
                registry.next_id = id;
            }
            let mut log = VecDeque::new();
            if let Some(text) = record.log.filter(|text| !text.is_empty()) {
                for line in text.split('\n') {
                    log.push_back(line.to_string());
                }
            }
            registry.records.push_back(TaskRecord {
                entry: TaskEntry {
                    id,
                    label: record.label,
                    kind: record.kind,
                    status: record.status,
                    started_at: record.started_at,
                    finished_at: record.finished_at,
                    cancellable: false,
                },
                key: String::new(),
                log,
                cancel: None,
            });
        }
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Debug, Clone)]
pub struct ServerSettings {
    pub host: String,
    pub port: u16,
    pub shutdown_grace: Duration,
    pub activity_timeout: Duration,
    pub http1_keep_alive: bool,
    pub http2_keep_alive: Option<Duration>,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            host: String::from("0.0.0.0"),
            port: 0,
            shutdown_grace: Duration::from_secs(20),
            activity_timeout: Duration::from_secs(180),
            http1_keep_alive: true,
            http2_keep_alive: None,
        }
    }
}

pub struct ServerHandle {
    pub addr: SocketAddr,
    state: Arc<AppState>,
    shutdown: watch::Sender<bool>,
    shutdown_grace: Duration,
}

impl ServerHandle {
    pub fn url(&self) -> String {
        if self.addr.ip().is_unspecified() {
            format!("http://localhost:{}", self.addr.port())
        } else {
            format!("http://{}", self.addr)
        }
    }

    pub fn start_build(&self, target: String) -> Result<i64, String> {
        bitbake_build::start_build(self.state.clone(), target)
    }

    pub fn warm_up(&self, image: Option<String>) {
        let state = self.state.clone();
        tokio::spawn(async move {
            let cancel = Arc::new(AtomicBool::new(false));
            if let Ok(task) = state.try_begin(
                "Startup: images, layers, environment".to_string(),
                "metadata",
                "metadata",
                Some(cancel.clone()),
                false,
            ) {
                let ok =
                    metadata::project_metadata(&state, task, &cancel, &state.working_directory, false)
                        .await
                        .is_ok();
                state.end_task(task, if ok { "done" } else { "failed" });
            }

            if let Some(image) = image {
                let cancel = Arc::new(AtomicBool::new(false));
                let key = format!("deptree:{image}");
                if let Ok(task) = state.try_begin(
                    format!("Startup: dependency tree + task count · {image}"),
                    "deptree",
                    &key,
                    Some(cancel.clone()),
                    false,
                ) {
                    let ok = metadata::dependency_graph(
                        &state,
                        task,
                        &cancel,
                        &state.working_directory,
                        &image,
                        false,
                    )
                    .await
                    .is_ok();
                    state.end_task(task, if ok { "done" } else { "failed" });
                }
            }
        });
    }

    pub async fn shutdown(&self) {
        let _ = self.shutdown.send(true);

        let build_running = self.state.build.lock().unwrap().is_running();
        if build_running {
            info!("Cancelling active build before exit");
            self.state.build.lock().unwrap().request_cancel();
        }

        let cancelled_tasks = self.state.cancel_all_running_tasks();
        if cancelled_tasks > 0 {
            info!("Signalled {cancelled_tasks} background task(s) to cancel");
        }

        if build_running {
            let deadline = Instant::now() + self.shutdown_grace;
            loop {
                if !self.state.build.lock().unwrap().is_running() {
                    info!("Active build terminated cleanly");
                    break;
                }
                if Instant::now() >= deadline {
                    warn!(
                        "Build did not stop within {}s; the bitbake process will be killed on exit",
                        self.shutdown_grace.as_secs()
                    );
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

pub async fn spawn(working_directory: PathBuf, settings: ServerSettings) -> Result<ServerHandle> {
    let listener = TcpListener::bind((settings.host.as_str(), settings.port))
        .await
        .with_context(|| format!("failed to bind {}:{}", settings.host, settings.port))?;
    let addr = listener.local_addr()?;

    let (outbound, _) = broadcast::channel(256);
    let state = Arc::new(AppState {
        working_directory,
        outbound,
        poll_buffer: Mutex::new(PollBuffer::default()),
        build: Mutex::new(bitbake_build::BuildManager::default()),
        tasks: Mutex::new(TaskRegistry::default()),
        activity_timeout: settings.activity_timeout,
    });

    state.load_activity_history();

    let shutdown_grace = settings.shutdown_grace;
    let http_settings = Arc::new(settings);
    let (shutdown, shutdown_rx) = watch::channel(false);

    let accept_state = state.clone();
    let mut accept_shutdown = shutdown_rx.clone();
    tokio::spawn(async move {
        loop {
            let (stream, _peer_address) = tokio::select! {
                accepted = listener.accept() => match accepted {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        warn!("accept failed: {error}");
                        continue;
                    }
                },
                _ = accept_shutdown.changed() => {
                    debug!("shutdown signalled; no longer accepting connections");
                    break;
                }
            };
            let io = TokioIo::new(stream);
            let connection_state = accept_state.clone();
            let connection_settings = http_settings.clone();
            let mut connection_shutdown = shutdown_rx.clone();
            tokio::spawn(async move {
                let service = service_fn(move |request| handle(request, connection_state.clone()));
                let mut builder = auto::Builder::new(TokioExecutor::new());
                builder.http1().keep_alive(connection_settings.http1_keep_alive);
                if let Some(interval) = connection_settings.http2_keep_alive {
                    builder.http2().keep_alive_interval(interval);
                }
                let connection = builder.serve_connection(io, service);
                tokio::pin!(connection);
                tokio::select! {
                    result = connection.as_mut() => {
                        if let Err(error) = result {
                            debug!("connection error: {error}");
                        }
                    }
                    _ = connection_shutdown.changed() => {
                        connection.as_mut().graceful_shutdown();
                        if let Err(error) = connection.await {
                            debug!("connection error during shutdown: {error}");
                        }
                    }
                }
            });
        }
    });

    Ok(ServerHandle {
        addr,
        state,
        shutdown,
        shutdown_grace,
    })
}
