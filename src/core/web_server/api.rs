use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use log::{debug, warn};
use serde::Deserialize;
use serde_json::{Value, json};

use super::bitbake_build;
use super::metadata;
use super::{AppState, BeginError, ResponseBody, ServerMessage, json, text, unix_now};
use crate::core::cli::{
    BitForgeConfig, LockManager, run_dependency_add, run_dependency_delink, run_dependency_relink,
    run_dependency_remove,
};
use crate::core::conf::{
    enabled_layers, ensure_build_conf, read_layer_priority, read_local_conf_value,
    set_local_conf_value,
};
use crate::core::store;
use crate::utils::git_helper;

const WKS_SIZE_LIMIT: u64 = 1024 * 1024;

pub(crate) async fn handle_domain(
    request: Request<Incoming>,
    state: &Arc<AppState>,
    method: &Method,
    path: &str,
) -> Option<Response<ResponseBody>> {
    let query = request.uri().query().unwrap_or("").to_string();
    if method == &Method::GET {
        if let Some(build_id) = build_log_id(path) {
            return Some(get_build_log(state, build_id));
        }
        if let Some(task_id) = task_detail_id(path) {
            return Some(get_task_detail(state, task_id));
        }
    }
    if method == &Method::POST {
        if let Some(task_id) = task_cancel_id(path) {
            return Some(post_task_cancel(state, task_id));
        }
        if path == "/api/tasks/cancel-all" {
            return Some(post_tasks_cancel_all(state));
        }
    }
    match (method, path) {
        (&Method::GET, "/api/project") => Some(get_project(state)),
        (&Method::POST, "/api/project") => Some(post_project(request, state).await),
        (&Method::GET, "/api/dependencies") => Some(get_dependencies(state)),
        (&Method::POST, "/api/dependency") => Some(post_dependency(request, state).await),
        (&Method::GET, "/api/metadata") => Some(get_metadata(state, &query).await),
        (&Method::GET, "/api/layout") => Some(get_layout(state, &query).await),
        (&Method::GET, "/api/deptree") => Some(get_deptree(state, &query).await),
        (&Method::GET, "/api/recipetree") => Some(get_recipe_tree(state, &query).await),
        (&Method::GET, "/api/plan") => Some(get_plan(state, &query)),
        (&Method::GET, "/api/artifacts") => Some(get_artifacts(state)),
        (&Method::POST, "/api/layout/preview") => Some(post_layout_preview(request, state).await),
        (&Method::POST, "/api/layout/save") => Some(post_layout_save(request, state).await),
        (&Method::GET, "/api/builds") => Some(get_builds(state)),
        (&Method::POST, "/api/build") => Some(post_build(state)),
        (&Method::POST, "/api/build/cancel") => Some(post_build_cancel(state)),
        (&Method::GET, "/api/build/tasks") => Some(get_build_tasks(state)),
        (&Method::GET, "/api/tasks") => Some(get_tasks(state)),
        (&Method::GET, "/api/revisions") => Some(get_revisions(state)),
        (&Method::GET, "/api/plugins") => Some(get_plugins()),
        (&Method::GET, "/api/system") => Some(get_system()),
        (&Method::GET, "/api/cache") => Some(get_cache(state)),
        (&Method::POST, "/api/cache/clear") => Some(post_cache_clear(state)),
        _ => Some(json(StatusCode::NOT_FOUND, "{\"error\":\"not found\"}")),
    }
}

const CACHE_ENTRIES: [(&str, &str, &str); 4] = [
    ("downloads", "Downloads (DL_DIR)", "build/downloads"),
    ("sstate", "Shared state cache (SSTATE_DIR)", "build/sstate-cache"),
    ("tmp", "Build output (TMPDIR)", "build/tmp"),
    ("buildhistory", "Build history", "build/buildhistory"),
];

fn dir_stats(path: &Path) -> (u64, u64) {
    let mut bytes = 0;
    let mut files = 0;
    let Ok(entries) = std::fs::read_dir(path) else {
        return (0, 0);
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        match entry.file_type() {
            Ok(file_type) if file_type.is_dir() => {
                let (child_bytes, child_files) = dir_stats(&entry_path);
                bytes += child_bytes;
                files += child_files;
            }
            Ok(file_type) if file_type.is_file() => {
                if let Ok(metadata) = entry.metadata() {
                    bytes += metadata.len();
                }
                files += 1;
            }
            _ => {}
        }
    }
    (bytes, files)
}

fn compute_cache(root: &Path) -> Value {
    let mut entries = Vec::new();
    let mut total = 0u64;
    let mut downloads = 0u64;
    let mut cache = 0u64;
    for (key, label, relative) in CACHE_ENTRIES {
        let path = root.join(relative);
        let (bytes, files) = if path.exists() {
            dir_stats(&path)
        } else {
            (0, 0)
        };
        total += bytes;
        if key == "downloads" {
            downloads += bytes;
        } else if key == "sstate" || key == "tmp" {
            cache += bytes;
        }
        entries.push(json!({
            "key": key,
            "label": label,
            "path": relative,
            "exists": path.exists(),
            "bytes": bytes,
            "files": files,
        }));
    }
    json!({
        "entries": entries,
        "total_bytes": total,
        "downloads_bytes": downloads,
        "cache_bytes": cache,
        "updated_at": unix_now(),
    })
}

fn empty_cache() -> Value {
    let entries: Vec<Value> = CACHE_ENTRIES
        .iter()
        .map(|(key, label, relative)| {
            json!({ "key": key, "label": label, "path": relative, "exists": false, "bytes": 0, "files": 0 })
        })
        .collect();
    json!({
        "entries": entries,
        "total_bytes": 0,
        "downloads_bytes": 0,
        "cache_bytes": 0,
        "updated_at": Value::Null,
    })
}

fn spawn_cache_scan(state: &Arc<AppState>) {
    let Ok(task) = state.try_begin(
        "Scanning cache & downloads".to_string(),
        "cache",
        "cache-scan",
        None,
        false,
    ) else {
        return;
    };
    let state = state.clone();
    tokio::spawn(async move {
        let root = state.working_directory.clone();
        let result = tokio::task::spawn_blocking(move || compute_cache(&root)).await;
        match result {
            Ok(value) => {
                let _ = store::store_metadata(
                    &state.working_directory,
                    "cache-stats",
                    "current",
                    &value.to_string(),
                    unix_now(),
                );
                state.publish(ServerMessage::new("cache", value));
                state.end_task(task, "done");
            }
            Err(error) => {
                state.append_task_log(task, &error.to_string());
                state.end_task(task, "failed");
            }
        }
    });
}

fn get_cache(state: &Arc<AppState>) -> Response<ResponseBody> {
    let root = &state.working_directory;
    let cached = store::cached_metadata(root, "cache-stats", "current").ok().flatten();
    let scanning = state.running_task_for_key("cache-scan").is_some();
    let age = cached
        .as_ref()
        .and_then(|payload| serde_json::from_str::<Value>(payload).ok())
        .and_then(|value| value.get("updated_at").and_then(Value::as_i64))
        .map(|updated| unix_now().saturating_sub(updated));
    let stale = age.map(|seconds| seconds > 30).unwrap_or(true);
    if !scanning && stale {
        spawn_cache_scan(state);
    }
    let mut body = cached
        .and_then(|payload| serde_json::from_str::<Value>(&payload).ok())
        .unwrap_or_else(empty_cache);
    body["scanning"] = json!(scanning || stale);
    json(StatusCode::OK, &body.to_string())
}

fn post_cache_clear(state: &Arc<AppState>) -> Response<ResponseBody> {
    let root = &state.working_directory;
    let mut cleared = Vec::new();
    for (key, _label, relative) in CACHE_ENTRIES {
        let path = root.join(relative);
        if path.exists() {
            match std::fs::remove_dir_all(&path) {
                Ok(()) => cleared.push(key),
                Err(error) => return internal_error(&error.to_string()),
            }
        }
    }
    spawn_cache_scan(state);
    json(StatusCode::OK, &json!({ "cleared": cleared }).to_string())
}

fn get_plugins() -> Response<ResponseBody> {
    let body = json!({ "plugins": crate::core::plugins::names() });
    json(StatusCode::OK, &body.to_string())
}

fn get_system() -> Response<ResponseBody> {
    let cpus = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1);
    let effective = crate::core::conf::effective_threads();
    let (ram_total, ram_available, swap_total, swap_free) = read_meminfo();
    let body = json!({
        "cpus": cpus,
        "reserved_threads": 1,
        "effective_threads": effective,
        "ram_bytes": ram_total,
        "ram_available_bytes": ram_available,
        "swap_bytes": swap_total,
        "swap_free_bytes": swap_free,
        "recommended_bb_threads": effective,
        "recommended_parallel_make": format!("-j {effective}"),
    });
    json(StatusCode::OK, &body.to_string())
}

fn read_meminfo() -> (u64, u64, u64, u64) {
    let mut total = 0;
    let mut available = 0;
    let mut swap_total = 0;
    let mut swap_free = 0;
    if let Ok(contents) = std::fs::read_to_string("/proc/meminfo") {
        for line in contents.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                total = parse_kib(rest);
            } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
                available = parse_kib(rest);
            } else if let Some(rest) = line.strip_prefix("SwapTotal:") {
                swap_total = parse_kib(rest);
            } else if let Some(rest) = line.strip_prefix("SwapFree:") {
                swap_free = parse_kib(rest);
            }
        }
    }
    (total, available, swap_total, swap_free)
}

fn parse_kib(value: &str) -> u64 {
    value
        .split_whitespace()
        .next()
        .and_then(|number| number.parse::<u64>().ok())
        .map(|kib| kib * 1024)
        .unwrap_or(0)
}

fn get_revisions(state: &Arc<AppState>) -> Response<ResponseBody> {
    let root = &state.working_directory;
    let current = git_helper::project_status(root).ok().map(|status| {
        let changed: Vec<Value> = status
            .changed
            .iter()
            .map(|file| json!({ "status": file.status, "path": file.path }))
            .collect();
        json!({ "commit": status.commit, "branch": status.branch, "changed": changed })
    });
    let history = store::list_revisions(root, 50).unwrap_or_default();
    let body = json!({ "current": current, "history": history });
    json(StatusCode::OK, &body.to_string())
}

fn get_tasks(state: &Arc<AppState>) -> Response<ResponseBody> {
    let snapshot = state.tasks_snapshot();
    json(
        StatusCode::OK,
        &serde_json::to_string(&snapshot).unwrap_or_else(|_| "[]".to_string()),
    )
}

fn task_detail_id(path: &str) -> Option<u64> {
    let rest = path.strip_prefix("/api/tasks/")?;
    if rest.contains('/') {
        return None;
    }
    rest.parse().ok()
}

fn task_cancel_id(path: &str) -> Option<u64> {
    path.strip_prefix("/api/tasks/")?.strip_suffix("/cancel")?.parse().ok()
}

fn get_task_detail(state: &Arc<AppState>, id: u64) -> Response<ResponseBody> {
    match state.task_detail(id) {
        Some((entry, log)) => {
            let body = json!({
                "id": entry.id,
                "label": entry.label,
                "kind": entry.kind,
                "status": entry.status,
                "started_at": entry.started_at,
                "finished_at": entry.finished_at,
                "cancellable": entry.cancellable,
                "log": log,
            });
            json(StatusCode::OK, &body.to_string())
        }
        None => text(StatusCode::NOT_FOUND, "task not found"),
    }
}

fn post_task_cancel(state: &Arc<AppState>, id: u64) -> Response<ResponseBody> {
    let cancelled = state.cancel_task(id);
    json(StatusCode::OK, &json!({ "cancelled": cancelled }).to_string())
}

fn post_tasks_cancel_all(state: &Arc<AppState>) -> Response<ResponseBody> {
    let cancelled = state.cancel_all_running_tasks();
    json(StatusCode::OK, &json!({ "cancelled": cancelled }).to_string())
}

fn task_status<T>(cancel: &AtomicBool, result: &Result<T, anyhow::Error>) -> &'static str {
    if cancel.load(std::sync::atomic::Ordering::SeqCst) {
        "cancelled"
    } else if result.is_ok() {
        "done"
    } else {
        "failed"
    }
}

fn already_running(id: u64, label: String) -> Response<ResponseBody> {
    json(
        StatusCode::CONFLICT,
        &json!({ "running": true, "task_id": id, "label": label }).to_string(),
    )
}

fn begin_error_response(error: BeginError) -> Response<ResponseBody> {
    match error {
        BeginError::Running { id, label } => already_running(id, label),
        BeginError::Blocked { message } => {
            json(StatusCode::TOO_MANY_REQUESTS, &json!({ "error": message }).to_string())
        }
    }
}

const BUILD_ACTIVE_NOTE: &str =
    "A build is running. BitBake processes one task at a time, so these are cached values — details refresh automatically once the build completes.";

fn build_active(state: &Arc<AppState>) -> bool {
    state.build.lock().unwrap().is_running()
}

async fn wait_for_key(state: &Arc<AppState>, key: &str) {
    while state.running_task_for_key(key).is_some() {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }
}

fn build_active_response(value: Option<Value>) -> Response<ResponseBody> {
    let mut body = value.unwrap_or_else(|| json!({}));
    if let Value::Object(map) = &mut body {
        map.insert("build_active".to_string(), Value::Bool(true));
        map.entry("note")
            .or_insert_with(|| Value::String(BUILD_ACTIVE_NOTE.to_string()));
    }
    json(StatusCode::OK, &body.to_string())
}

const CONFIG_KEYS: [(&str, &str); 4] = [
    ("package_classes", "PACKAGE_CLASSES"),
    ("bb_number_threads", "BB_NUMBER_THREADS"),
    ("parallel_make", "PARALLEL_MAKE"),
    ("image_fstypes", "IMAGE_FSTYPES"),
];

fn get_project(state: &Arc<AppState>) -> Response<ResponseBody> {
    let config = match load_config(state) {
        Ok(config) => config,
        Err(response) => return response,
    };
    let bitbake = config
        .workspace
        .as_ref()
        .map(|workspace| workspace.bitbake.clone())
        .unwrap_or_default();
    let machine = read_local_conf_value(&state.working_directory, "MACHINE").unwrap_or_default();
    let distro = read_local_conf_value(&state.working_directory, "DISTRO").unwrap_or_default();
    let mut body = json!({
        "name": config.display_name(),
        "distro": distro,
        "machine": machine,
        "target_image": config.default_image().unwrap_or_default(),
        "yocto_release": config.effective_release(),
        "bitbake": bitbake,
    });
    for (field, conf_key) in CONFIG_KEYS {
        let value = read_local_conf_value(&state.working_directory, conf_key).unwrap_or_default();
        body[field] = Value::String(value);
    }
    json(StatusCode::OK, &body.to_string())
}

#[derive(Deserialize)]
struct ProjectUpdate {
    distro: Option<String>,
    machine: Option<String>,
    target_image: Option<String>,
    yocto_release: Option<String>,
    package_classes: Option<String>,
    bb_number_threads: Option<String>,
    parallel_make: Option<String>,
    image_fstypes: Option<String>,
}

async fn post_project(
    request: Request<Incoming>,
    state: &Arc<AppState>,
) -> Response<ResponseBody> {
    let update: ProjectUpdate = match read_json(request).await {
        Ok(value) => value,
        Err(response) => return response,
    };

    let mut config = match load_config(state) {
        Ok(config) => config,
        Err(response) => return response,
    };
    if let Some(distro) = update.distro {
        if let Err(error) = set_local_conf_value(&state.working_directory, "DISTRO", &distro) {
            return internal_error(&error.to_string());
        }
    }
    if let Some(machine) = update.machine {
        if let Err(error) = set_local_conf_value(&state.working_directory, "MACHINE", &machine) {
            return internal_error(&error.to_string());
        }
    }
    if let Some(target_image) = update.target_image {
        config.default = Some(crate::core::cli::DefaultSection { image: target_image });
    }
    if let Some(release) = update.yocto_release {
        config
            .workspace
            .get_or_insert_with(Default::default)
            .yocto_release = Some(release);
    }

    let optional_settings = [
        ("PACKAGE_CLASSES", update.package_classes),
        ("BB_NUMBER_THREADS", update.bb_number_threads),
        ("PARALLEL_MAKE", update.parallel_make),
        ("IMAGE_FSTYPES", update.image_fstypes),
    ];
    for (conf_key, value) in optional_settings {
        if let Some(value) = value {
            if let Err(error) = set_local_conf_value(&state.working_directory, conf_key, &value) {
                return internal_error(&error.to_string());
            }
        }
    }

    if let Err(error) = config.save() {
        return internal_error(&error.to_string());
    }
    if let Err(error) = ensure_build_conf(&state.working_directory, &config) {
        return internal_error(&error.to_string());
    }

    get_project(state)
}

fn get_dependencies(state: &Arc<AppState>) -> Response<ResponseBody> {
    let config = match load_config(state) {
        Ok(config) => config,
        Err(response) => return response,
    };

    let enabled = enabled_layers(&config);
    let layers: Vec<Value> = enabled
        .iter()
        .filter(|layer| layer.dependency.is_none())
        .map(|layer| {
            json!({ "name": layer.name, "path": layer.path, "priority": layer.priority })
        })
        .collect();

    let lock = LockManager::load(&state.working_directory).ok();
    let dependencies: Vec<Value> = enabled
        .iter()
        .filter(|layer| layer.dependency.is_some())
        .map(|layer| {
            let gitname = layer.dependency.clone().unwrap_or_default();
            let layername = layer.subpath.clone().unwrap_or_default();
            let layer_dep = config
                .dependency
                .get(&gitname)
                .and_then(|layers| layers.get(&layername));
            let locked = lock
                .as_ref()
                .and_then(|lock| lock.data.layers.get(&gitname))
                .and_then(|layers| layers.get(&layername));
            let priority = read_layer_priority(&state.working_directory.join(&layer.path));
            let commit = locked
                .map(|entry| entry.commit.clone())
                .or_else(|| layer_dep.and_then(|dependency| dependency.commit.clone()));
            let short_commit = commit
                .as_deref()
                .map(|value| value.chars().take(12).collect::<String>());
            json!({
                "name": layer.name,
                "repository": gitname,
                "path": layer.path,
                "git": layer_dep.map(|dependency| dependency.giturl.clone()).unwrap_or_default(),
                "branch": layer_dep.and_then(|dependency| dependency.branch.clone()),
                "tag": layer_dep.and_then(|dependency| dependency.tag.clone()),
                "commit": short_commit,
                "priority": priority,
                "linked": layer.linked,
            })
        })
        .collect();

    let body = json!({
        "layers": layers,
        "dependencies": dependencies,
    });
    json(StatusCode::OK, &body.to_string())
}

fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| pair.strip_prefix(&format!("{key}=")))
}

async fn get_deptree(state: &Arc<AppState>, query: &str) -> Response<ResponseBody> {
    let config = match load_config(state) {
        Ok(config) => config,
        Err(response) => return response,
    };
    let image = query_value(query, "image")
        .map(decode_component)
        .or_else(|| config.default_image().map(str::to_string));
    let Some(image) = image else {
        return json(StatusCode::BAD_REQUEST, "{\"error\":\"no image; set a default image\"}");
    };
    let refresh = query_value(query, "refresh") == Some("1");
    let force = query_value(query, "force") == Some("1");
    let key = format!("deptree:{image}");
    if build_active(state) {
        return build_active_response(metadata::cached_value(&state.working_directory, &key));
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let task = match state.try_begin(
        format!("BitBake: dependency tree · {image}"),
        "deptree",
        &key,
        Some(cancel.clone()),
        force,
    ) {
        Ok(id) => id,
        Err(error) => return begin_error_response(error),
    };
    let result =
        metadata::dependency_graph(state, task, &cancel, &state.working_directory, &image, refresh)
            .await;
    state.end_task(task, task_status(&cancel, &result));
    match result {
        Ok(value) => json(StatusCode::OK, &value.to_string()),
        Err(error) => internal_error(&error.to_string()),
    }
}

fn recipe_of_task(task: &str) -> String {
    match task.rfind(".do_") {
        Some(index) => task[..index].to_string(),
        None => task.to_string(),
    }
}

fn extract_quoted(value: &str) -> Option<String> {
    let start = value.find('"')?;
    let rest = &value[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn parse_recipe_adjacency(
    dot_path: &Path,
) -> std::collections::BTreeMap<String, std::collections::BTreeSet<String>> {
    use std::collections::{BTreeMap, BTreeSet};
    let mut adjacency: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let contents = match std::fs::read_to_string(dot_path) {
        Ok(contents) => contents,
        Err(error) => {
            warn!(
                target: "recipetree",
                "unable to read dependency graph {}: {error}",
                dot_path.display()
            );
            return adjacency;
        }
    };
    for line in contents.lines() {
        let trimmed = line.trim();
        let Some(arrow) = trimmed.find("->") else {
            continue;
        };
        let (Some(source), Some(destination)) =
            (extract_quoted(&trimmed[..arrow]), extract_quoted(&trimmed[arrow + 2..]))
        else {
            continue;
        };
        let from = recipe_of_task(&source);
        let to = recipe_of_task(&destination);
        adjacency.entry(from.clone()).or_default();
        adjacency.entry(to.clone()).or_default();
        if from != to {
            adjacency.entry(from).or_default().insert(to);
        }
    }
    debug!(target: "recipetree", "parsed {} recipes from {}", adjacency.len(), dot_path.display());
    adjacency
}

fn build_status_rank(status: &str) -> u8 {
    match status {
        "cached" => 1,
        "built" => 2,
        "running" => 3,
        "failed" => 4,
        _ => 0,
    }
}

fn recipe_build_status(state: &Arc<AppState>) -> std::collections::BTreeMap<String, String> {
    use std::collections::BTreeMap;
    let rows = state.build.lock().unwrap().task_table_json();
    let mut map: BTreeMap<String, (u8, String)> = BTreeMap::new();
    if let Value::Array(array) = rows {
        for row in array {
            let recipe = row.get("recipe").and_then(Value::as_str).unwrap_or("").to_string();
            let status = row.get("status").and_then(Value::as_str).unwrap_or("").to_string();
            if recipe.is_empty() {
                continue;
            }
            let rank = build_status_rank(&status);
            let entry = map.entry(recipe).or_insert((0, status.clone()));
            if rank >= entry.0 {
                *entry = (rank, status);
            }
        }
    }
    map.into_iter().map(|(name, (_, status))| (name, status)).collect()
}

async fn get_recipe_tree(state: &Arc<AppState>, query: &str) -> Response<ResponseBody> {
    let config = match load_config(state) {
        Ok(config) => config,
        Err(response) => return response,
    };
    let image = query_value(query, "image")
        .map(decode_component)
        .or_else(|| config.default_image().map(str::to_string));
    let Some(image) = image else {
        return json(StatusCode::BAD_REQUEST, "{\"error\":\"no image; set a default image\"}");
    };
    let refresh = query_value(query, "refresh") == Some("1");
    let root = state.working_directory.clone();

    if build_active(state) {
        let dot = metadata::graph_dot_path(&root, &image);
        let plan = metadata::cached_plan(&root, &image);
        let Some(plan) = plan.filter(|_| dot.exists()) else {
            return json(
                StatusCode::OK,
                &json!({
                    "root": image,
                    "recipes": {},
                    "stats": { "total": 0, "dirty": 0, "clean": 0 },
                    "build_active": true,
                    "note": BUILD_ACTIVE_NOTE,
                })
                .to_string(),
            );
        };
        let recipe_layers = metadata::cached_value(&root, "metadata")
            .and_then(|value| value.get("recipe_layers").and_then(Value::as_object).cloned())
            .unwrap_or_default();
        let adjacency = parse_recipe_adjacency(&dot);
        let body = assemble_recipe_tree(&image, &adjacency, &plan, &recipe_layers, state, true);
        return json(StatusCode::OK, &body.to_string());
    }

    let dep_key = format!("deptree:{image}");
    if state.running_task_for_key(&dep_key).is_some() {
        wait_for_key(state, &dep_key).await;
    } else {
        let cancel = Arc::new(AtomicBool::new(false));
        let task = state.begin_task(
            format!("BitBake: dependency tree · {image}"),
            "deptree",
            &dep_key,
            Some(cancel.clone()),
        );
        let graph = metadata::dependency_graph(state, task, &cancel, &root, &image, refresh).await;
        state.end_task(task, task_status(&cancel, &graph));
        if let Err(error) = graph {
            return internal_error(&error.to_string());
        }
    }
    if !metadata::graph_dot_path(&root, &image).exists() {
        return internal_error("dependency graph unavailable");
    }

    let plan_key = format!("plan:{image}");
    let plan = if state.running_task_for_key(&plan_key).is_some() {
        wait_for_key(state, &plan_key).await;
        match metadata::cached_plan(&root, &image) {
            Some(value) => value,
            None => return internal_error("rebuild plan unavailable"),
        }
    } else {
        let cancel = Arc::new(AtomicBool::new(false));
        let task = state.begin_task(
            format!("BitBake: rebuild plan · {image}"),
            "plan",
            &plan_key,
            Some(cancel.clone()),
        );
        let plan = metadata::recipe_plan(state, task, &cancel, &root, &image, refresh).await;
        state.end_task(task, task_status(&cancel, &plan));
        match plan {
            Ok(value) => value,
            Err(error) => return internal_error(&error.to_string()),
        }
    };

    let recipe_layers = if state.running_task_for_key("metadata").is_some() {
        wait_for_key(state, "metadata").await;
        metadata::cached_value(&root, "metadata")
    } else {
        let cancel = Arc::new(AtomicBool::new(false));
        let task = state.begin_task(
            "BitBake: recipe layer map".to_string(),
            "metadata",
            "metadata",
            Some(cancel.clone()),
        );
        let metadata = metadata::project_metadata(state, task, &cancel, &root, refresh).await;
        state.end_task(task, task_status(&cancel, &metadata));
        match metadata {
            Ok(value) => Some(value),
            Err(error) => {
                warn!(target: "recipetree", "recipe layer map unavailable: {error}");
                None
            }
        }
    }
    .and_then(|value| value.get("recipe_layers").and_then(Value::as_object).cloned())
    .unwrap_or_default();

    let adjacency = parse_recipe_adjacency(&metadata::graph_dot_path(&root, &image));
    let body = assemble_recipe_tree(&image, &adjacency, &plan, &recipe_layers, state, false);
    json(StatusCode::OK, &body.to_string())
}

fn assemble_recipe_tree(
    image: &str,
    adjacency: &std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    plan: &Value,
    recipe_layers: &serde_json::Map<String, Value>,
    state: &Arc<AppState>,
    build_active: bool,
) -> Value {
    let dirty = value_str_set(plan, "dirty");
    let clean = value_str_set(plan, "clean");
    let build_status = recipe_build_status(state);

    let mut recipes = serde_json::Map::new();
    let mut dirty_count = 0u64;
    let mut clean_count = 0u64;
    for (name, deps) in adjacency {
        let status = if let Some(status) = build_status.get(name) {
            status.clone()
        } else if dirty.contains(name) {
            "dirty".to_string()
        } else if clean.contains(name) {
            "cached".to_string()
        } else {
            String::new()
        };
        match status.as_str() {
            "dirty" | "built" | "running" => dirty_count += 1,
            "cached" => clean_count += 1,
            _ => {}
        }
        let layer = recipe_layers.get(name).and_then(Value::as_str).unwrap_or("");
        recipes.insert(
            name.clone(),
            json!({
                "deps": deps.iter().cloned().collect::<Vec<_>>(),
                "status": status,
                "layer": layer,
            }),
        );
    }

    let mut body = json!({
        "root": image,
        "recipes": recipes,
        "stats": { "total": adjacency.len(), "dirty": dirty_count, "clean": clean_count },
    });
    if build_active {
        if let Value::Object(map) = &mut body {
            map.insert("build_active".to_string(), Value::Bool(true));
            map.insert("note".to_string(), Value::String(BUILD_ACTIVE_NOTE.to_string()));
        }
    }
    body
}

fn value_str_set(value: &Value, key: &str) -> std::collections::BTreeSet<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn get_plan(state: &Arc<AppState>, query: &str) -> Response<ResponseBody> {
    let config = match load_config(state) {
        Ok(config) => config,
        Err(response) => return response,
    };
    let image = query_value(query, "image")
        .map(decode_component)
        .or_else(|| config.default_image().map(str::to_string));
    let Some(image) = image else {
        return json(StatusCode::BAD_REQUEST, "{\"error\":\"no image\"}");
    };
    if let Some(plan) = metadata::cached_plan(&state.working_directory, &image) {
        let dirty = plan.get("dirty").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
        let clean = plan.get("clean").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
        return json(
            StatusCode::OK,
            &json!({
                "image": image,
                "computed": true,
                "dirty": dirty,
                "clean": clean,
                "total": dirty + clean,
                "build_active": build_active(state),
            })
            .to_string(),
        );
    }
    if build_active(state) {
        return json(
            StatusCode::OK,
            &json!({ "image": image, "computed": false, "build_active": true, "note": BUILD_ACTIVE_NOTE }).to_string(),
        );
    }
    if state.running_task_for_key(&format!("plan:{image}")).is_none() {
        spawn_plan(state, &image);
    }
    json(StatusCode::OK, &json!({ "image": image, "computed": false }).to_string())
}

fn spawn_plan(state: &Arc<AppState>, image: &str) {
    let cancel = Arc::new(AtomicBool::new(false));
    let key = format!("plan:{image}");
    let Ok(task) = state.try_begin(
        format!("BitBake: rebuild plan · {image}"),
        "plan",
        &key,
        Some(cancel.clone()),
        false,
    ) else {
        return;
    };
    let state = state.clone();
    let image = image.to_string();
    tokio::spawn(async move {
        let root = state.working_directory.clone();
        let result = metadata::recipe_plan(&state, task, &cancel, &root, &image, false).await;
        state.end_task(task, task_status(&cancel, &result));
    });
}

fn package_recipe(file_stem: &str) -> String {
    let mut parts = Vec::new();
    for segment in file_stem.split('-') {
        if segment.chars().next().map(|character| character.is_ascii_digit()).unwrap_or(false) {
            break;
        }
        parts.push(segment);
    }
    if parts.is_empty() {
        file_stem.to_string()
    } else {
        parts.join("-")
    }
}

fn collect_packages(dir: &Path, ext: &str, out: &mut Vec<Value>, limit: usize) {
    if out.len() >= limit {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            debug!(target: "artifacts", "package feed directory absent: {}", dir.display());
            return;
        }
        Err(error) => {
            warn!(target: "artifacts", "failed to read package feed {}: {error}", dir.display());
            return;
        }
    };
    for entry in entries.flatten() {
        if out.len() >= limit {
            warn!(target: "artifacts", "package scan hit limit {limit}, truncating results");
            return;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_packages(&path, ext, out, limit);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some(ext) {
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(error) => {
                    warn!(target: "artifacts", "stat failed for {}: {error}", path.display());
                    continue;
                }
            };
            let file_name = entry.file_name().to_string_lossy().to_string();
            let stem = file_name.trim_end_matches(&format!(".{ext}"));
            out.push(json!({
                "name": file_name,
                "recipe": package_recipe(stem),
                "bytes": metadata.len(),
            }));
        }
    }
}

fn get_artifacts(state: &Arc<AppState>) -> Response<ResponseBody> {
    let deploy = state.working_directory.join("build").join("tmp").join("deploy");
    debug!(target: "artifacts", "scanning deploy directory {}", deploy.display());
    let mut images = Vec::new();
    let images_root = deploy.join("images");
    match std::fs::read_dir(&images_root) {
        Ok(machines) => {
            for machine in machines.flatten() {
                let machine_path = machine.path();
                if !machine_path.is_dir() {
                    continue;
                }
                let machine_name = machine.file_name().to_string_lossy().to_string();
                let files = match std::fs::read_dir(&machine_path) {
                    Ok(files) => files,
                    Err(error) => {
                        warn!(target: "artifacts", "failed to read machine images {}: {error}", machine_path.display());
                        continue;
                    }
                };
                for file in files.flatten() {
                    let metadata = match file.metadata() {
                        Ok(metadata) => metadata,
                        Err(error) => {
                            warn!(target: "artifacts", "stat failed for {}: {error}", file.path().display());
                            continue;
                        }
                    };
                    if !metadata.is_file() || metadata.file_type().is_symlink() {
                        continue;
                    }
                    images.push(json!({
                        "name": file.file_name().to_string_lossy(),
                        "machine": machine_name,
                        "bytes": metadata.len(),
                    }));
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            debug!(target: "artifacts", "deploy images directory absent: {}", images_root.display());
        }
        Err(error) => {
            warn!(target: "artifacts", "failed to read deploy images {}: {error}", images_root.display());
        }
    }
    let mut packages = Vec::new();
    for (folder, ext) in [("rpm", "rpm"), ("deb", "deb"), ("ipk", "ipk")] {
        collect_packages(&deploy.join(folder), ext, &mut packages, 3000);
    }
    debug!(
        target: "artifacts",
        "deploy scan complete: {} images, {} packages",
        images.len(),
        packages.len()
    );
    let body = json!({ "images": images, "packages": packages });
    json(StatusCode::OK, &body.to_string())
}

async fn get_metadata(state: &Arc<AppState>, query: &str) -> Response<ResponseBody> {
    if let Err(response) = load_config(state) {
        return response;
    }
    if build_active(state) {
        return build_active_response(metadata::cached_value(&state.working_directory, "metadata"));
    }
    let refresh = query_value(query, "refresh") == Some("1");
    let force = query_value(query, "force") == Some("1");
    let cancel = Arc::new(AtomicBool::new(false));
    let task = match state.try_begin(
        "BitBake: images, layers, environment".to_string(),
        "metadata",
        "metadata",
        Some(cancel.clone()),
        force,
    ) {
        Ok(id) => id,
        Err(error) => return begin_error_response(error),
    };
    let result =
        metadata::project_metadata(state, task, &cancel, &state.working_directory, refresh).await;
    state.end_task(task, task_status(&cancel, &result));
    match result {
        Ok(value) => json(StatusCode::OK, &value.to_string()),
        Err(error) => internal_error(&error.to_string()),
    }
}

async fn get_layout(state: &Arc<AppState>, query: &str) -> Response<ResponseBody> {
    let config = match load_config(state) {
        Ok(config) => config,
        Err(response) => return response,
    };
    let Some(image) = query_value(query, "image").map(decode_component) else {
        return json(StatusCode::BAD_REQUEST, "{\"error\":\"image query required\"}");
    };
    let refresh = query_value(query, "refresh") == Some("1");
    let force = query_value(query, "force") == Some("1");
    let key = format!("layout:{image}");
    if build_active(state) {
        let cached = metadata::cached_value(&state.working_directory, &key).map(|mut value| {
            annotate_layout(&mut value, &config, &state.working_directory);
            value
        });
        return build_active_response(cached);
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let task = match state.try_begin(
        format!("BitBake: resolve WKS · {image}"),
        "layout",
        &key,
        Some(cancel.clone()),
        force,
    ) {
        Ok(id) => id,
        Err(error) => return begin_error_response(error),
    };
    let result =
        metadata::image_layout(state, task, &cancel, &state.working_directory, &image, refresh)
            .await;
    state.end_task(task, task_status(&cancel, &result));
    match result {
        Ok(mut value) => {
            annotate_layout(&mut value, &config, &state.working_directory);
            json(StatusCode::OK, &value.to_string())
        }
        Err(error) => internal_error(&error.to_string()),
    }
}

#[derive(Deserialize)]
struct LayoutPreview {
    image: String,
    content: String,
}

async fn post_layout_preview(
    request: Request<Incoming>,
    state: &Arc<AppState>,
) -> Response<ResponseBody> {
    if let Err(response) = load_config(state) {
        return response;
    }
    let input: LayoutPreview = match read_json(request).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let key = format!("preview:{}", input.image);
    let task = state
        .try_begin(
            format!("BitBake: parse WKS preview · {}", input.image),
            "layout",
            &key,
            Some(cancel.clone()),
            true,
        )
        .unwrap_or_else(|_| unreachable!("force begin never fails"));
    let result = metadata::preview_layout(
        state,
        task,
        &cancel,
        &state.working_directory,
        &input.image,
        &input.content,
    )
    .await;
    state.end_task(task, task_status(&cancel, &result));
    match result {
        Ok(value) => json(StatusCode::OK, &value.to_string()),
        Err(error) => internal_error(&error.to_string()),
    }
}

#[derive(Deserialize)]
struct LayoutSave {
    image: String,
    content: String,
    target_layer: Option<String>,
}

async fn post_layout_save(
    request: Request<Incoming>,
    state: &Arc<AppState>,
) -> Response<ResponseBody> {
    let config = match load_config(state) {
        Ok(config) => config,
        Err(response) => return response,
    };
    let input: LayoutSave = match read_json(request).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if input.content.len() as u64 > WKS_SIZE_LIMIT {
        return json(StatusCode::BAD_REQUEST, "{\"error\":\"wks content too large\"}");
    }

    let root = state.working_directory.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let key = format!("save:{}", input.image);
    let task = state
        .try_begin(
            format!("BitBake: save WKS · {}", input.image),
            "layout",
            &key,
            Some(cancel.clone()),
            true,
        )
        .unwrap_or_else(|_| unreachable!("force begin never fails"));
    let layout = match metadata::image_layout(state, task, &cancel, &root, &input.image, false).await
    {
        Ok(value) => value,
        Err(error) => {
            state.end_task(task, task_status(&cancel, &Err::<(), _>(anyhow::anyhow!(""))));
            return internal_error(&error.to_string());
        }
    };
    let source_path = layout.get("source_path").and_then(Value::as_str).map(PathBuf::from);
    let own_layers = own_project_layers(&config, &root);

    let target_path = if let Some(target) = &input.target_layer {
        match own_layers.iter().find(|(name, _)| name == target) {
            Some((_, path)) => wks_destination(path, &input.image),
            None => {
                state.end_task(task, "failed");
                return json(StatusCode::BAD_REQUEST, "{\"error\":\"unknown target layer\"}");
            }
        }
    } else if let Some(source) = source_path
        .as_ref()
        .filter(|path| own_layers.iter().any(|(_, layer)| path.starts_with(layer)))
    {
        source.clone()
    } else {
        match own_layers.first() {
            Some((_, path)) => wks_destination(path, &input.image),
            None => {
                state.end_task(task, "failed");
                return json(
                    StatusCode::BAD_REQUEST,
                    "{\"error\":\"no project layer to save into\"}",
                );
            }
        }
    };

    if !target_path.starts_with(&root) {
        state.end_task(task, "failed");
        return json(StatusCode::BAD_REQUEST, "{\"error\":\"refusing to write outside the project\"}");
    }
    if let Some(parent) = target_path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            state.end_task(task, "failed");
            return internal_error(&error.to_string());
        }
    }
    if let Err(error) = std::fs::write(&target_path, &input.content) {
        state.end_task(task, "failed");
        return internal_error(&error.to_string());
    }
    if let Some(name) = target_path.file_name().and_then(|value| value.to_str()) {
        if let Err(error) = set_local_conf_value(&root, "WKS_FILE", name) {
            state.end_task(task, "failed");
            return internal_error(&error.to_string());
        }
    }

    let refreshed = metadata::image_layout(state, task, &cancel, &root, &input.image, true).await;
    state.end_task(task, task_status(&cancel, &refreshed));
    match refreshed {
        Ok(mut value) => {
            annotate_layout(&mut value, &config, &root);
            json(StatusCode::OK, &value.to_string())
        }
        Err(error) => internal_error(&error.to_string()),
    }
}

fn own_project_layers(config: &BitForgeConfig, root: &Path) -> Vec<(String, PathBuf)> {
    enabled_layers(config)
        .into_iter()
        .filter(|layer| layer.dependency.is_none())
        .map(|layer| (layer.name, root.join(&layer.path)))
        .collect()
}

fn wks_destination(layer_path: &Path, image: &str) -> PathBuf {
    layer_path.join("wic").join(format!("{image}.wks"))
}

fn annotate_layout(value: &mut Value, config: &BitForgeConfig, root: &Path) {
    let own_layers = own_project_layers(config, root);
    let source_in_project = value
        .get("source_path")
        .and_then(Value::as_str)
        .map(|source| own_layers.iter().any(|(_, path)| Path::new(source).starts_with(path)))
        .unwrap_or(false);
    let names: Vec<Value> = own_layers
        .iter()
        .map(|(name, _)| Value::String(name.clone()))
        .collect();
    let default_layer = own_layers.first().map(|(name, _)| name.clone());
    if let Value::Object(map) = value {
        map.insert("editable_in_place".to_string(), Value::Bool(source_in_project));
        map.insert("project_layers".to_string(), Value::Array(names));
        map.insert(
            "default_layer".to_string(),
            default_layer.map(Value::String).unwrap_or(Value::Null),
        );
    }
}

fn decode_component(value: &str) -> String {
    value.replace("%2F", "/").replace("%2f", "/").replace('+', " ")
}

#[derive(Deserialize)]
struct DependencyAction {
    action: String,
    spec: Option<String>,
    name: Option<String>,
    layer: Option<String>,
}

async fn post_dependency(
    request: Request<Incoming>,
    state: &Arc<AppState>,
) -> Response<ResponseBody> {
    let action: DependencyAction = match read_json(request).await {
        Ok(value) => value,
        Err(response) => return response,
    };

    let working_directory = state.working_directory.clone();
    let result = match action.action.as_str() {
        "add" => {
            let Some(spec) = action.spec else {
                return json(StatusCode::BAD_REQUEST, "{\"error\":\"spec required\"}");
            };
            let task = state.begin_task(
                format!("git: add dependency · {spec}"),
                "dependency",
                &format!("dep-add:{spec}"),
                None,
            );
            let outcome =
                tokio::task::spawn_blocking(move || run_dependency_add(&working_directory, &spec))
                    .await;
            state.end_task(task, if matches!(&outcome, Ok(Ok(_))) { "done" } else { "failed" });
            outcome
        }
        "remove" | "delink" | "relink" => {
            let target = match action.action.as_str() {
                "remove" => action.name.clone(),
                _ => action.layer.clone(),
            };
            let Some(target) = target else {
                return json(StatusCode::BAD_REQUEST, "{\"error\":\"target required\"}");
            };
            let verb = action.action.clone();
            let task = state.begin_task(
                format!("dependency {verb}: {target}"),
                "dependency",
                &format!("dep-{verb}:{target}"),
                None,
            );
            let working = working_directory.clone();
            let target_for_task = target.clone();
            let outcome = tokio::task::spawn_blocking(move || match verb.as_str() {
                "delink" => run_dependency_delink(&working, &target_for_task),
                "relink" => run_dependency_relink(&working, &target_for_task),
                _ => run_dependency_remove(&working, &target_for_task),
            })
            .await;
            state.end_task(task, if matches!(&outcome, Ok(Ok(_))) { "done" } else { "failed" });
            outcome
        }
        other => {
            return json(
                StatusCode::BAD_REQUEST,
                &json!({ "error": format!("unknown action '{other}'") }).to_string(),
            );
        }
    };

    match result {
        Ok(Ok(())) => get_dependencies(state),
        Ok(Err(error)) => json(
            StatusCode::BAD_REQUEST,
            &json!({ "error": error.to_string() }).to_string(),
        ),
        Err(error) => internal_error(&error.to_string()),
    }
}

fn get_builds(state: &Arc<AppState>) -> Response<ResponseBody> {
    let active = state.build.lock().unwrap().current_snapshot();
    let history = store::list_builds(&state.working_directory, 50).unwrap_or_default();
    let body = json!({ "active": active, "history": history });
    json(StatusCode::OK, &body.to_string())
}

fn build_log_id(path: &str) -> Option<i64> {
    path.strip_prefix("/api/builds/")?.strip_suffix("/log")?.parse().ok()
}

fn get_build_log(state: &Arc<AppState>, build_id: i64) -> Response<ResponseBody> {
    let record = match store::get_build(&state.working_directory, build_id) {
        Ok(Some(record)) => record,
        Ok(None) => return text(StatusCode::NOT_FOUND, "build not found"),
        Err(error) => return internal_error(&error.to_string()),
    };
    let log_path = match record.log_path {
        Some(path) => path,
        None => return text(StatusCode::NOT_FOUND, "no log for this build"),
    };
    match std::fs::read_to_string(&log_path) {
        Ok(contents) => text(StatusCode::OK, &contents),
        Err(_) => text(StatusCode::NOT_FOUND, "log file is unavailable"),
    }
}

fn post_build(state: &Arc<AppState>) -> Response<ResponseBody> {
    let config = match load_config(state) {
        Ok(config) => config,
        Err(response) => return response,
    };
    let target = match config.default_image() {
        Some(image) => image.to_string(),
        None => {
            return json(
                StatusCode::BAD_REQUEST,
                "{\"error\":\"no default image set; add [default] image or build a target\"}",
            );
        }
    };

    match bitbake_build::start_build(state.clone(), target) {
        Ok(build_id) => json(StatusCode::OK, &json!({ "id": build_id }).to_string()),
        Err(error) => json(StatusCode::CONFLICT, &json!({ "error": error }).to_string()),
    }
}

fn post_build_cancel(state: &Arc<AppState>) -> Response<ResponseBody> {
    let cancelled = state.build.lock().unwrap().request_cancel();
    json(StatusCode::OK, &json!({ "cancelled": cancelled }).to_string())
}

fn get_build_tasks(state: &Arc<AppState>) -> Response<ResponseBody> {
    let tasks = state.build.lock().unwrap().task_table_json();
    json(StatusCode::OK, &tasks.to_string())
}

fn load_config(state: &Arc<AppState>) -> Result<BitForgeConfig, Response<ResponseBody>> {
    if !BitForgeConfig::exists_in(&state.working_directory) {
        return Err(json(
            StatusCode::NOT_FOUND,
            "{\"error\":\"no BitForge project in this directory\"}",
        ));
    }
    BitForgeConfig::load_from(&state.working_directory)
        .map_err(|error| internal_error(&error.to_string()))
}

async fn read_json<T: serde::de::DeserializeOwned>(
    request: Request<Incoming>,
) -> Result<T, Response<ResponseBody>> {
    let bytes = request
        .into_body()
        .collect()
        .await
        .map_err(|error| internal_error(&error.to_string()))?
        .to_bytes();
    serde_json::from_slice(&bytes)
        .map_err(|_| json(StatusCode::BAD_REQUEST, "{\"error\":\"invalid json body\"}"))
}

fn internal_error(message: &str) -> Response<ResponseBody> {
    json(
        StatusCode::INTERNAL_SERVER_ERROR,
        &json!({ "error": message }).to_string(),
    )
}
