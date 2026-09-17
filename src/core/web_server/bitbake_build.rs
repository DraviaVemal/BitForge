use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use log::{debug, error};
use tokio::process::Command;
use tokio::sync::mpsc;
use xml::reader::{EventReader, XmlEvent};

use super::{AppState, ServerMessage};
use crate::core::store;
use crate::core::workspace::{BUILD_DIR, FORGE_SOURCE_DIR};

const RECENT_DONE_LIMIT: usize = 20;
const BUILDS_DIR: &str = "builds";

#[derive(Debug, Clone, Serialize)]
pub struct RunningTask {
    pub recipe: String,
    pub task: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskRow {
    pub recipe: String,
    pub task: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskGroup {
    pub task: String,
    pub running: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TaskView {
    pub total: u64,
    pub done: u64,
    pub planned: u64,
    pub running: u64,
    pub failed: u64,
    pub setscene_total: u64,
    pub setscene_covered: u64,
    pub processing: Vec<RunningTask>,
    pub recent_done: Vec<String>,
    pub groups: Vec<TaskGroup>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildSnapshot {
    pub id: i64,
    pub status: String,
    pub target: String,
    pub overall_progress: u8,
    pub current_task: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub tasks: TaskView,
}

#[derive(Default)]
pub struct BuildManager {
    current: Option<BuildSnapshot>,
    cancel_flag: Option<Arc<AtomicBool>>,
    task_id: Option<u64>,
    task_table: BTreeMap<String, TaskRow>,
}

impl BuildManager {
    pub fn is_running(&self) -> bool {
        self.current
            .as_ref()
            .map(|snapshot| snapshot.status == "running")
            .unwrap_or(false)
    }

    pub fn request_cancel(&self) -> bool {
        match &self.cancel_flag {
            Some(flag) => {
                flag.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    pub fn current_snapshot(&self) -> Option<BuildSnapshot> {
        self.current.clone()
    }

    pub fn task_table_json(&self) -> Value {
        let mut rows: Vec<&TaskRow> = self.task_table.values().collect();
        rows.sort_by(|left, right| {
            status_order(&left.status)
                .cmp(&status_order(&right.status))
                .then(left.recipe.cmp(&right.recipe))
                .then(left.task.cmp(&right.task))
        });
        Value::Array(
            rows.into_iter()
                .filter_map(|row| serde_json::to_value(row).ok())
                .collect(),
        )
    }

    fn active_json(&self) -> Value {
        match &self.current {
            Some(snapshot) => serde_json::to_value(snapshot).unwrap_or(Value::Null),
            None => json!({ "status": "idle" }),
        }
    }
}

pub fn start_build(state: Arc<AppState>, target: String) -> Result<i64, String> {
    {
        let manager = state.build.lock().unwrap();
        if manager.is_running() {
            return Err("a build is already running".to_string());
        }
    }

    state.cancel_bitbake_activities();

    let root = state.working_directory.clone();
    let started_at = now();
    let _ = crate::core::tracking::record_revision(&root, "build");
    let report_dir = root
        .join(FORGE_SOURCE_DIR)
        .join(BUILDS_DIR)
        .join(started_at.to_string());
    let log_path = report_dir.join("build.log");

    let build_id = store::insert_build(
        &root,
        &target,
        started_at,
        &report_dir.to_string_lossy(),
        &log_path.to_string_lossy(),
    )
    .map_err(|error| error.to_string())?;

    let cancel_flag = Arc::new(AtomicBool::new(false));
    {
        let mut manager = state.build.lock().unwrap();
        manager.current = Some(BuildSnapshot {
            id: build_id,
            status: "running".to_string(),
            target: target.clone(),
            overall_progress: 0,
            current_task: "Starting bitbake".to_string(),
            started_at,
            finished_at: None,
            tasks: TaskView::default(),
        });
        manager.cancel_flag = Some(cancel_flag.clone());
        manager.task_table.clear();
    }
    {
        let task = state.begin_task(
            format!("BitBake: build · {target}"),
            "build",
            &format!("build:{target}"),
            Some(cancel_flag.clone()),
        );
        state.build.lock().unwrap().task_id = Some(task);
    }
    publish_build(&state);

    let task_state = state.clone();
    tokio::spawn(async move {
        run_bitbake(task_state, build_id, target, report_dir, log_path, cancel_flag).await
    });
    Ok(build_id)
}

async fn run_bitbake(
    state: Arc<AppState>,
    build_id: i64,
    target: String,
    report_dir: PathBuf,
    log_path: PathBuf,
    cancel: Arc<AtomicBool>,
) {
    let root = state.working_directory.clone();

    if let Err(error) = prepare_report_dir(&root, &report_dir) {
        finalize_failed(&state, build_id, &format!("prepare report dir: {error}"));
        return;
    }
    if let Err(error) = prepare_build_dir(&root) {
        finalize_failed(&state, build_id, &format!("prepare build dir: {error}"));
        return;
    }

    let mut log_writer = File::create(&log_path).ok().map(BufWriter::new);

    let build_dir = root.join(BUILD_DIR);
    let bitbake_bin = root
        .join(FORGE_SOURCE_DIR)
        .join("bitbake")
        .join("bin")
        .join("bitbake");
    if !bitbake_bin.exists() {
        finalize_failed(&state, build_id, "bitbake is missing from ForgeSource/bitbake");
        return;
    }
    let script = match crate::core::plugins::prepare(&root) {
        Ok(path) => path,
        Err(error) => {
            finalize_failed(&state, build_id, &format!("failed to prepare build plugin: {error}"));
            return;
        }
    };

    if state.bitbake_activity_running() {
        note(&state, build_id, "Waiting for background BitBake tasks to stop…");
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while state.bitbake_activity_running() && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    let mut command = Command::new("python3");
    command
        .arg(&script)
        .arg(&root)
        .arg("build")
        .arg(&target)
        .current_dir(&build_dir)
        .env("BUILDDIR", &build_dir)
        .env("PATH", build_path(&root))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            finalize_failed(&state, build_id, &format!("failed to start bitbake: {error}"));
            return;
        }
    };

    let (line_tx, mut line_rx) = mpsc::channel::<(bool, String)>(256);
    if let Some(stdout) = child.stdout.take() {
        spawn_reader(stdout, true, line_tx.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_reader(stderr, false, line_tx.clone());
    }
    drop(line_tx);

    let mut cancelled = false;
    loop {
        tokio::select! {
            line = line_rx.recv() => match line {
                Some((is_stdout, line)) => {
                    if let Some(payload) = line.strip_prefix("BITFORGE_EVENT:") {
                        apply_event(&state, build_id, payload);
                        continue;
                    }
                    if line.strip_prefix("BITFORGE_METADATA:").is_some() {
                        continue;
                    }
                    if is_stdout {
                        debug!(target: "bitbake", "{line}");
                    } else {
                        error!(target: "bitbake", "{line}");
                    }
                    if let Some(writer) = log_writer.as_mut() {
                        let _ = writeln!(writer, "{line}");
                    }
                }
                None => break,
            },
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                if cancel.load(Ordering::SeqCst) {
                    cancelled = true;
                    let _ = child.start_kill();
                }
            }
        }
    }

    if let Some(mut writer) = log_writer {
        let _ = writer.flush();
    }

    let status = child.wait().await;
    let final_status = if cancelled || cancel.load(Ordering::SeqCst) {
        "cancelled"
    } else if status.map(|code| code.success()).unwrap_or(false) {
        "success"
    } else {
        "failed"
    };

    let collected = collect_build_stats(&root, &report_dir);
    if !collected.is_empty() {
        note(
            &state,
            build_id,
            &format!("Collected stats into report dir: {}", collected.join(", ")),
        );
    }
    finalize(&state, build_id, final_status);
}

fn collect_build_stats(root: &Path, report_dir: &Path) -> Vec<String> {
    let build = root.join(BUILD_DIR);
    let mut collected = Vec::new();
    for (source, name) in [
        (build.join("tmp").join("buildstats"), "buildstats"),
        (build.join("buildhistory"), "buildhistory"),
    ] {
        if source.exists() && copy_dir_recursive(&source, &report_dir.join(name)).is_ok() {
            collected.push(name.to_string());
        }
    }
    collected
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let target = destination.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else {
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

fn spawn_reader<R>(reader: R, is_stdout: bool, sender: mpsc::Sender<(bool, String)>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if sender.send((is_stdout, line)).await.is_err() {
                break;
            }
        }
    });
}

fn apply_event(state: &Arc<AppState>, build_id: i64, payload: &str) {
    let Some(attributes) = parse_event_element(payload) else {
        return;
    };
    let kind = attributes.get("kind").map(String::as_str).unwrap_or("");
    let task_id;
    {
        let mut manager = state.build.lock().unwrap();
        task_id = manager.task_id;
        let mut table_update: Option<(String, TaskRow)> = None;
        {
            let Some(current) = manager.current.as_mut() else {
                return;
            };
            if current.id != build_id {
                return;
            }
            match kind {
                "progress" => {
                    let total = attr_u64(&attributes, "total");
                    let completed = attr_u64(&attributes, "completed");
                    let active = attr_u64(&attributes, "active");
                    current.tasks.total = total;
                    current.tasks.done = completed;
                    current.tasks.running = active;
                    current.tasks.failed = attr_u64(&attributes, "failed");
                    current.tasks.planned = total.saturating_sub(completed);
                    current.tasks.setscene_total = attr_u64(&attributes, "setscene_total");
                    current.tasks.setscene_covered = attr_u64(&attributes, "setscene_covered");
                    if total > 0 {
                        current.overall_progress =
                            ((completed as f64 / total as f64) * 100.0).min(100.0) as u8;
                    }
                }
                "task" => {
                    let state_name = attributes.get("state").cloned().unwrap_or_default();
                    let recipe = attributes.get("recipe").cloned().unwrap_or_default();
                    let task = attributes.get("task").cloned().unwrap_or_default();
                    match state_name.as_str() {
                        "started" => {
                            if !current
                                .tasks
                                .processing
                                .iter()
                                .any(|running| running.recipe == recipe && running.task == task)
                            {
                                current.tasks.processing.push(RunningTask {
                                    recipe: recipe.clone(),
                                    task: task.clone(),
                                });
                            }
                            current.current_task = format!("{recipe} {task}");
                        }
                        "completed" | "failed" => {
                            current.tasks.processing.retain(|running| {
                                !(running.recipe == recipe && running.task == task)
                            });
                            current.tasks.recent_done.push(format!("{recipe} {task}"));
                            while current.tasks.recent_done.len() > RECENT_DONE_LIMIT {
                                current.tasks.recent_done.remove(0);
                            }
                        }
                        _ => {}
                    }
                    current.tasks.groups = group_running(&current.tasks.processing);
                    let key = format!("{recipe}\u{1}{task}");
                    let status = table_status(&state_name).to_string();
                    let timestamp = now();
                    let started_at = matches!(state_name.as_str(), "started" | "setscene-started")
                        .then_some(timestamp);
                    let finished_at =
                        matches!(status.as_str(), "built" | "cached" | "failed").then_some(timestamp);
                    let signature = attributes
                        .get("taskhash")
                        .filter(|value| !value.is_empty())
                        .cloned();
                    table_update = Some((
                        key,
                        TaskRow {
                            recipe,
                            task,
                            status,
                            started_at,
                            finished_at,
                            signature,
                        },
                    ));
                }
                _ => {}
            }
        }
        if let Some((key, row)) = table_update {
            upsert_task_row(&mut manager.task_table, key, row);
        }
    }
    if let Some(task_id) = task_id {
        if kind == "task" {
            if let Some(text) = event_task_log(&attributes) {
                state.append_task_log(task_id, &text);
            }
        }
    }
    publish_build(state);
}

fn note(state: &Arc<AppState>, build_id: i64, message: &str) {
    let task_id;
    {
        let mut manager = state.build.lock().unwrap();
        task_id = manager.task_id;
        let Some(current) = manager.current.as_mut() else {
            return;
        };
        if current.id != build_id {
            return;
        }
        current.current_task = message.to_string();
    }
    if let Some(task_id) = task_id {
        state.append_task_log(task_id, message);
    }
    publish_build(state);
}

fn event_task_log(attributes: &BTreeMap<String, String>) -> Option<String> {
    let state_name = attributes.get("state")?;
    let recipe = attributes.get("recipe").cloned().unwrap_or_default();
    let task = attributes.get("task").cloned().unwrap_or_default();
    Some(format!("{state_name}: {recipe} {task}"))
}

fn table_status(state_name: &str) -> &'static str {
    match state_name {
        "started" => "running",
        "completed" => "built",
        "failed" => "failed",
        "setscene-completed" => "cached",
        _ => "queued",
    }
}

fn status_rank(status: &str) -> u8 {
    match status {
        "queued" => 0,
        "running" => 1,
        "cached" | "built" | "failed" => 2,
        _ => 0,
    }
}

fn status_order(status: &str) -> u8 {
    match status {
        "running" => 0,
        "failed" => 1,
        "queued" => 2,
        "built" => 3,
        "cached" => 4,
        _ => 5,
    }
}

fn upsert_task_row(table: &mut BTreeMap<String, TaskRow>, key: String, mut row: TaskRow) {
    let new_rank = status_rank(&row.status);
    if let Some(existing) = table.get(&key) {
        if row.started_at.is_none() {
            row.started_at = existing.started_at;
        }
        if row.signature.is_none() {
            row.signature = existing.signature.clone();
        }
        if status_rank(&existing.status) > new_rank {
            return;
        }
    }
    table.insert(key, row);
}

fn group_running(processing: &[RunningTask]) -> Vec<TaskGroup> {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for running in processing {
        *counts.entry(running.task.clone()).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(task, running)| TaskGroup { task, running })
        .collect()
}

fn parse_event_element(payload: &str) -> Option<BTreeMap<String, String>> {
    let reader = EventReader::from_str(payload);
    for event in reader {
        if let Ok(XmlEvent::StartElement { attributes, .. }) = event {
            let mut map = BTreeMap::new();
            for attribute in attributes {
                map.insert(attribute.name.local_name, attribute.value);
            }
            return Some(map);
        }
    }
    None
}

fn attr_u64(attributes: &BTreeMap<String, String>, key: &str) -> u64 {
    attributes
        .get(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

fn prepare_report_dir(root: &Path, report_dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(report_dir)?;
    let source_conf = root.join("conf");
    for file in ["local.conf", "bblayers.conf"] {
        let source = source_conf.join(file);
        if source.exists() {
            fs::copy(&source, report_dir.join(file))?;
        }
    }
    Ok(())
}

pub(crate) fn prepare_build_dir(root: &Path) -> std::io::Result<()> {
    let source_conf = root.join("conf");
    let build_conf = root.join(BUILD_DIR).join("conf");
    fs::create_dir_all(&build_conf)?;
    for file in ["local.conf", "bblayers.conf"] {
        let source = source_conf.join(file);
        if source.exists() {
            crate::core::conf::copy_generated_conf(&source, &build_conf.join(file))?;
        }
    }
    crate::core::conf::ensure_tracking_inherits(&build_conf.join("local.conf"))?;
    crate::core::conf::ensure_parallelism(&build_conf.join("local.conf"))?;
    Ok(())
}

pub(crate) fn build_path(root: &Path) -> String {
    let existing = std::env::var("PATH").unwrap_or_default();
    let mut entries = vec![root.join(FORGE_SOURCE_DIR).join("bitbake").join("bin")];

    let layers_dir = root.join(FORGE_SOURCE_DIR).join("layers");
    if let Ok(read_dir) = std::fs::read_dir(&layers_dir) {
        for entry in read_dir.flatten() {
            let scripts = entry.path().join("scripts");
            if scripts.is_dir() {
                entries.push(scripts);
            }
        }
    }

    let mut parts: Vec<String> = entries.iter().map(|path| path.display().to_string()).collect();
    parts.push(existing);
    parts.join(":")
}

fn finalize_failed(state: &Arc<AppState>, build_id: i64, message: &str) {
    {
        let mut manager = state.build.lock().unwrap();
        if let Some(current) = manager.current.as_mut() {
            if current.id == build_id {
                current.current_task = message.to_string();
            }
        }
    }
    finalize(state, build_id, "failed");
}

fn finalize(state: &Arc<AppState>, build_id: i64, status: &str) {
    let finished_at = now();
    let mut started_at = finished_at;
    let task_id;
    {
        let mut manager = state.build.lock().unwrap();
        if let Some(current) = manager.current.as_mut() {
            if current.id == build_id {
                current.status = status.to_string();
                current.finished_at = Some(finished_at);
                started_at = current.started_at;
                if status == "success" {
                    current.overall_progress = 100;
                }
            }
        }
        manager.cancel_flag = None;
        task_id = manager.task_id.take();
    }

    if let Some(task_id) = task_id {
        state.end_task(task_id, if status == "success" { "done" } else { status });
    }

    let _ = store::finish_build(
        &state.working_directory,
        build_id,
        status,
        finished_at,
        finished_at - started_at,
    );
    publish_build(state);
}

fn publish_build(state: &Arc<AppState>) {
    let payload = state.build.lock().unwrap().active_json();
    state.publish(ServerMessage::new("build", payload));
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
