use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde_json::json;

use crate::core::store;
use crate::utils::git_helper;

pub fn require_git_project(root: &Path) -> Result<()> {
    git_helper::ensure_git_project(root)
}

pub fn ensure_git_project(root: &Path) -> Result<()> {
    git_helper::ensure_repo_initialized(root)
}

pub fn record_revision(root: &Path, event: &str) -> Result<()> {
    let status = git_helper::project_status(root)?;
    let files: Vec<_> = status
        .changed
        .iter()
        .map(|file| json!({ "status": file.status, "path": file.path }))
        .collect();
    let changed_files = serde_json::to_string(&files).unwrap_or_else(|_| "[]".to_string());
    store::record_revision(
        root,
        event,
        status.commit.as_deref(),
        status.branch.as_deref(),
        !status.changed.is_empty(),
        status.changed.len() as i64,
        &changed_files,
        now(),
    )
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
