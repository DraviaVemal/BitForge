use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use log::{debug, error};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use super::{AppState, bitbake_build};
use crate::core::plugins;
use crate::core::store;
use crate::core::workspace::{BUILD_DIR, FORGE_SOURCE_DIR, LAYERS_DIR};


pub async fn project_metadata(
    state: &Arc<AppState>,
    task: u64,
    cancel: &AtomicBool,
    root: &Path,
    refresh: bool,
) -> Result<Value> {
    run_cached(state, task, cancel, root, "metadata", "metadata", &[], None, refresh).await
}

pub async fn image_layout(
    state: &Arc<AppState>,
    task: u64,
    cancel: &AtomicBool,
    root: &Path,
    image: &str,
    refresh: bool,
) -> Result<Value> {
    let kind = format!("layout:{image}");
    run_cached(state, task, cancel, root, &kind, "layout", &[image], None, refresh).await
}

pub async fn recipe_plan(
    state: &Arc<AppState>,
    task: u64,
    cancel: &AtomicBool,
    root: &Path,
    image: &str,
    refresh: bool,
) -> Result<Value> {
    let kind = format!("plan:{image}");
    run_cached(state, task, cancel, root, &kind, "plan", &[image], None, refresh).await
}

pub async fn preview_layout(
    state: &Arc<AppState>,
    task: u64,
    cancel: &AtomicBool,
    root: &Path,
    image: &str,
    content: &str,
) -> Result<Value> {
    let stdin = json!({ "content": content }).to_string();
    run_python(state, task, cancel, root, "preview", &[image], Some(&stdin)).await
}

pub async fn dependency_graph(
    state: &Arc<AppState>,
    task: u64,
    cancel: &AtomicBool,
    root: &Path,
    image: &str,
    refresh: bool,
) -> Result<Value> {
    let kind = format!("deptree:{image}");
    let signature = compute_signature(root)?;
    if !refresh {
        if let Some(payload) = store::cached_metadata(root, &kind, &signature)? {
            if let Ok(value) = serde_json::from_str::<Value>(&payload) {
                state.append_task_log(task, "Using cached dependency graph; no BitBake run needed.");
                return Ok(value);
            }
        }
    }

    bitbake_build::prepare_build_dir(root).context("failed to prepare build/conf for graph")?;
    let build_dir = root.join(BUILD_DIR);
    let bitbake = root
        .join(FORGE_SOURCE_DIR)
        .join("bitbake")
        .join("bin")
        .join("bitbake");
    if !bitbake.exists() {
        bail!("bitbake is missing from ForgeSource/bitbake");
    }

    state.append_task_log(
        task,
        &format!("Running `bitbake -g {image}` for the full dependency tree and task count…"),
    );

    let mut command = Command::new(&bitbake);
    command
        .arg("-g")
        .arg(image)
        .current_dir(&build_dir)
        .env("BUILDDIR", &build_dir)
        .env("PATH", bitbake_build::build_path(root))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .kill_on_drop(true);

    let mut child = command.spawn().context("failed to launch bitbake -g")?;
    let (line_tx, mut line_rx) = mpsc::channel::<(bool, String)>(256);
    if let Some(stdout) = child.stdout.take() {
        spawn_reader(stdout, true, line_tx.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_reader(stderr, false, line_tx.clone());
    }
    drop(line_tx);

    let started = Instant::now();
    let mut timed_out = false;
    loop {
        tokio::select! {
            line = line_rx.recv() => match line {
                Some((is_stdout, line)) => {
                    if !line.trim().is_empty() {
                        if is_stdout {
                            debug!(target: "bitbake", "{line}");
                        } else {
                            error!(target: "bitbake", "{line}");
                        }
                        state.append_task_log(task, &line);
                    }
                }
                None => break,
            },
            _ = tokio::time::sleep(Duration::from_millis(200)) => {
                if cancel.load(Ordering::SeqCst) {
                    let _ = child.start_kill();
                }
                if !state.activity_timeout.is_zero() && started.elapsed() >= state.activity_timeout {
                    timed_out = true;
                    let _ = child.start_kill();
                }
            }
        }
    }

    let status = child.wait().await;
    if cancel.load(Ordering::SeqCst) {
        bail!("cancelled");
    }
    if timed_out {
        bail!("{}", activity_timeout_message("dependency graph", state.activity_timeout));
    }
    if !status.map(|code| code.success()).unwrap_or(false) {
        bail!("`bitbake -g` failed; see the task log for details");
    }

    let value = parse_dependency_graph(&build_dir, image)?;
    stash_graph_artifacts(root, &build_dir, image);
    store::store_metadata(root, &kind, &signature, &value.to_string(), now())?;
    Ok(value)
}

fn parse_dependency_graph(build_dir: &Path, image: &str) -> Result<Value> {
    let mut task_nodes: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut recipe_edges: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();

    if let Ok(contents) = fs::read_to_string(build_dir.join("task-depends.dot")) {
        for line in contents.lines() {
            if let Some((source, destination)) = parse_dot_edge(line) {
                let source_recipe = recipe_of(&source);
                let destination_recipe = recipe_of(&destination);
                task_nodes.insert(source);
                task_nodes.insert(destination);
                if source_recipe != destination_recipe {
                    recipe_edges.insert((source_recipe, destination_recipe));
                }
            } else if let Some(node) = parse_dot_node(line) {
                task_nodes.insert(node);
            }
        }
    }

    let recipes: Vec<String> = fs::read_to_string(build_dir.join("pn-buildlist"))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect();

    let edges: Vec<Value> = recipe_edges
        .iter()
        .take(5000)
        .map(|(from, to)| json!([from, to]))
        .collect();

    Ok(json!({
        "image": image,
        "total_tasks": task_nodes.len(),
        "recipe_count": recipes.len(),
        "edge_count": recipe_edges.len(),
        "recipes": recipes,
        "edges": edges,
    }))
}

fn parse_dot_edge(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    let arrow = line.find("->")?;
    let source = extract_quoted(&line[..arrow])?;
    let destination = extract_quoted(&line[arrow + 2..])?;
    Some((source, destination))
}

fn parse_dot_node(line: &str) -> Option<String> {
    let line = line.trim();
    if line.contains("->") || !line.starts_with('"') {
        return None;
    }
    extract_quoted(line)
}

fn extract_quoted(value: &str) -> Option<String> {
    let start = value.find('"')?;
    let rest = &value[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn recipe_of(task: &str) -> String {
    match task.rfind(".do_") {
        Some(index) => task[..index].to_string(),
        None => task.to_string(),
    }
}

fn stash_graph_artifacts(root: &Path, build_dir: &Path, image: &str) {
    let destination = root.join(FORGE_SOURCE_DIR).join("graph").join(graph_slug(image));
    let _ = fs::create_dir_all(&destination);
    for file in [
        "task-depends.dot",
        "pn-buildlist",
        "recipe-depends.dot",
        "package-depends.dot",
    ] {
        let source = build_dir.join(file);
        if source.exists() {
            let _ = fs::copy(&source, destination.join(file));
            let _ = fs::remove_file(&source);
        }
    }
}

pub fn graph_slug(image: &str) -> String {
    image
        .chars()
        .map(|character| if character.is_alphanumeric() || character == '-' || character == '_' { character } else { '_' })
        .collect()
}

pub fn graph_dot_path(root: &Path, image: &str) -> PathBuf {
    root.join(FORGE_SOURCE_DIR)
        .join("graph")
        .join(graph_slug(image))
        .join("task-depends.dot")
}

pub fn cached_plan(root: &Path, image: &str) -> Option<Value> {
    let signature = compute_signature(root).ok()?;
    let payload = store::cached_metadata(root, &format!("plan:{image}"), &signature)
        .ok()
        .flatten()?;
    serde_json::from_str(&payload).ok()
}

pub fn cached_value(root: &Path, kind: &str) -> Option<Value> {
    let signature = compute_signature(root).ok()?;
    let payload = store::cached_metadata(root, kind, &signature).ok().flatten()?;
    serde_json::from_str(&payload).ok()
}

#[allow(clippy::too_many_arguments)]
async fn run_cached(
    state: &Arc<AppState>,
    task: u64,
    cancel: &AtomicBool,
    root: &Path,
    kind: &str,
    action: &str,
    args: &[&str],
    stdin: Option<&str>,
    refresh: bool,
) -> Result<Value> {
    let signature = compute_signature(root)?;
    if !refresh {
        if let Some(payload) = store::cached_metadata(root, kind, &signature)? {
            if let Ok(value) = serde_json::from_str::<Value>(&payload) {
                state.append_task_log(task, "Using cached result; no BitBake run needed.");
                return Ok(value);
            }
        }
    }
    let value = run_python(state, task, cancel, root, action, args, stdin).await?;
    store::store_metadata(root, kind, &signature, &value.to_string(), now())?;
    Ok(value)
}

async fn run_python(
    state: &Arc<AppState>,
    task: u64,
    cancel: &AtomicBool,
    root: &Path,
    action: &str,
    args: &[&str],
    stdin: Option<&str>,
) -> Result<Value> {
    bitbake_build::prepare_build_dir(root).context("failed to prepare build/conf for metadata")?;
    let script_path = plugins::prepare(root)?;
    let build_dir = root.join(BUILD_DIR);

    state.append_task_log(task, &format!("Starting BitBake Tinfoil ({action})…"));

    let mut command = Command::new("python3");
    command
        .arg(&script_path)
        .arg(root)
        .arg(action)
        .args(args)
        .current_dir(&build_dir)
        .env("BUILDDIR", &build_dir)
        .env("PATH", bitbake_build::build_path(root))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .kill_on_drop(true);

    let mut child = command
        .spawn()
        .context("failed to launch python3 for bitbake metadata")?;

    if let (Some(payload), Some(mut handle)) = (stdin, child.stdin.take()) {
        handle.write_all(payload.as_bytes()).await.ok();
        drop(handle);
    }

    let (line_tx, mut line_rx) = mpsc::channel::<(bool, String)>(256);
    if let Some(stdout) = child.stdout.take() {
        spawn_reader(stdout, true, line_tx.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_reader(stderr, false, line_tx.clone());
    }
    drop(line_tx);

    let mut marker = None;
    let started = Instant::now();
    let mut timed_out = false;
    loop {
        tokio::select! {
            line = line_rx.recv() => match line {
                Some((is_stdout, line)) => {
                    if is_stdout {
                        if let Some(captured) = line.strip_prefix("BITFORGE_METADATA:") {
                            marker = Some(captured.to_string());
                            continue;
                        }
                    }
                    if !line.trim().is_empty() {
                        if is_stdout {
                            debug!(target: "tinfoil", "{line}");
                        } else {
                            error!(target: "tinfoil", "{line}");
                        }
                        state.append_task_log(task, &line);
                    }
                }
                None => break,
            },
            _ = tokio::time::sleep(Duration::from_millis(200)) => {
                if cancel.load(Ordering::SeqCst) {
                    let _ = child.start_kill();
                }
                if !state.activity_timeout.is_zero() && started.elapsed() >= state.activity_timeout {
                    timed_out = true;
                    let _ = child.start_kill();
                }
            }
        }
    }

    let _ = child.wait().await;
    if cancel.load(Ordering::SeqCst) {
        bail!("cancelled");
    }
    if timed_out {
        bail!("{}", activity_timeout_message(action, state.activity_timeout));
    }

    let Some(marker) = marker else {
        bail!("BitBake produced no result; see the task log for details");
    };
    let value: Value = serde_json::from_str(&marker).context("failed to parse metadata json")?;
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        bail!("{error}");
    }
    Ok(value)
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

fn compute_signature(root: &Path) -> Result<String> {
    let mut hasher = DefaultHasher::new();
    for relative in [
        "conf/local.conf",
        "conf/bblayers.conf",
        "BitForge.toml",
        "BitForge.lock",
    ] {
        hash_file(&mut hasher, &root.join(relative));
    }
    hash_tree(&mut hasher, &root.join(LAYERS_DIR));
    Ok(format!("{:016x}", hasher.finish()))
}

fn hash_file(hasher: &mut DefaultHasher, path: &Path) {
    if let Ok(contents) = fs::read(path) {
        path.to_string_lossy().hash(hasher);
        contents.hash(hasher);
    }
}

fn hash_tree(hasher: &mut DefaultHasher, directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|entry| entry.ok().map(|dir_entry| dir_entry.path())).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            hash_tree(hasher, &path);
        } else if let Ok(metadata) = fs::metadata(&path) {
            path.to_string_lossy().hash(hasher);
            metadata.len().hash(hasher);
            if let Ok(modified) = metadata.modified() {
                if let Ok(elapsed) = modified.duration_since(UNIX_EPOCH) {
                    elapsed.as_secs().hash(hasher);
                }
            }
        }
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn activity_timeout_message(action: &str, timeout: Duration) -> String {
    let minutes = timeout.as_secs().div_ceil(60);
    format!(
        "TIMEOUT: activity '{action}' did not finish within {minutes} min. Add `activity_timeout = <minutes>` under [server] in BitForge.toml to wait longer."
    )
}
