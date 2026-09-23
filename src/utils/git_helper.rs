use std::io::{IsTerminal, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, anyhow};
use git2::{
    Direction, FetchOptions, Object, Oid, Progress, Remote, RemoteCallbacks, Repository, Status,
    StatusOptions, build::CheckoutBuilder, build::RepoBuilder,
};

#[derive(Debug, Clone)]
pub enum GitRef {
    Branch(String),
    Tag(String),
    Commit(String),
}

#[derive(Debug, Clone)]
pub struct ChangedFile {
    pub status: String,
    pub path: String,
}

#[derive(Debug, Clone, Default)]
pub struct GitStatus {
    pub commit: Option<String>,
    pub branch: Option<String>,
    pub changed: Vec<ChangedFile>,
}

pub fn ensure_git_project(root: &Path) -> Result<()> {
    Repository::discover(root).map(|_| ()).map_err(|_| {
        anyhow!(
            "BitForge only works inside a git-tracked project. It uses git to detect which files \
             changed since the last commit so it can decide what work to redo. No git repository \
             was found at {}. Run `git init` here and commit your BitForge.toml before continuing.",
            root.display()
        )
    })
}

pub fn ensure_repo_initialized(root: &Path) -> Result<()> {
    if Repository::discover(root).is_ok() {
        return Ok(());
    }
    Repository::init(root)
        .with_context(|| format!("failed to initialize a git repository in {}", root.display()))?;
    Ok(())
}

pub fn project_status(root: &Path) -> Result<GitStatus> {
    let repo = Repository::discover(root)
        .map_err(|_| anyhow!("no git repository found at {}", root.display()))?;
    let head = repo.head().ok();
    let commit = head.as_ref().and_then(|head| head.target()).map(|oid| oid.to_string());
    let branch = head
        .as_ref()
        .and_then(|reference| reference.shorthand().ok())
        .map(str::to_string);

    let mut options = StatusOptions::new();
    options.include_untracked(true).recurse_untracked_dirs(true);
    let mut changed = Vec::new();
    if let Ok(statuses) = repo.statuses(Some(&mut options)) {
        for entry in statuses.iter() {
            let path = entry.path().unwrap_or_default().to_string();
            if path.is_empty() {
                continue;
            }
            changed.push(ChangedFile {
                status: describe_status(entry.status()),
                path,
            });
        }
    }
    Ok(GitStatus {
        commit,
        branch,
        changed,
    })
}

fn describe_status(status: Status) -> String {
    if status.intersects(Status::WT_NEW | Status::INDEX_NEW) {
        "added"
    } else if status.intersects(Status::WT_DELETED | Status::INDEX_DELETED) {
        "deleted"
    } else if status.intersects(Status::WT_RENAMED | Status::INDEX_RENAMED) {
        "renamed"
    } else if status.intersects(Status::WT_MODIFIED | Status::INDEX_MODIFIED) {
        "modified"
    } else {
        "changed"
    }
    .to_string()
}

pub struct GitHelper {
    repo: Repository,
}

impl GitHelper {
    pub fn remote_branch_exists(url: &str, branch: &str) -> Result<bool> {
        let mut remote =
            Remote::create_detached(url).with_context(|| format!("invalid git url {url}"))?;
        remote
            .connect(Direction::Fetch)
            .with_context(|| format!("failed to reach {url}"))?;
        let target = format!("refs/heads/{branch}");
        let exists = remote
            .list()
            .context("failed to list remote refs")?
            .iter()
            .any(|head| head.name() == target);
        let _ = remote.disconnect();
        Ok(exists)
    }

    pub fn clone_or_open(url: &str, dest: &Path) -> Result<Self> {
        let repo = if dest.join(".git").exists() {
            Repository::open(dest).with_context(|| format!("failed to open {}", dest.display()))?
        } else {
            clone_with_progress(url, dest)
                .with_context(|| format!("failed to clone {url} into {}", dest.display()))?
        };
        Ok(Self { repo })
    }

    pub fn fetch(&self) -> Result<()> {
        if let Ok(mut remote) = self.repo.find_remote("origin") {
            let show = std::io::stderr().is_terminal();
            let progressed = Arc::new(AtomicBool::new(false));
            let mut callbacks = RemoteCallbacks::new();
            if show {
                let flag = progressed.clone();
                callbacks.transfer_progress(move |stats| {
                    flag.store(true, Ordering::Relaxed);
                    print_fetch_progress(&stats);
                    true
                });
            }
            let mut options = FetchOptions::new();
            options.remote_callbacks(callbacks);
            remote
                .fetch::<&str>(&[], Some(&mut options), None)
                .context("git fetch failed")?;
            if show && progressed.load(Ordering::Relaxed) {
                eprintln!();
            }
        }
        Ok(())
    }

    pub fn checkout(&self, git_ref: &GitRef) -> Result<()> {
        let object = self.resolve_object(git_ref)?;
        self.repo
            .checkout_tree(&object, Some(CheckoutBuilder::new().force()))
            .context("failed to checkout tree")?;
        self.repo
            .set_head_detached(object.id())
            .context("failed to move HEAD")?;
        Ok(())
    }

    pub fn head_commit(&self) -> Result<String> {
        let head = self.repo.head().context("failed to read HEAD")?;
        let oid = head.target().ok_or_else(|| anyhow!("HEAD has no target"))?;
        Ok(oid.to_string())
    }

    pub fn ensure_worktree(&self, name: &str, path: &Path, git_ref: &GitRef) -> Result<String> {
        let object = self.resolve_object(git_ref)?;
        let oid = object.id();
        let ref_name = format!("refs/heads/forge/{name}");
        self.repo
            .reference(&ref_name, oid, true, "bitforge worktree")
            .with_context(|| format!("failed to create worktree ref {ref_name}"))?;

        if let Ok(worktree) = self.repo.find_worktree(name) {
            let worktree_repo = Repository::open_from_worktree(&worktree)
                .with_context(|| format!("failed to open worktree {name}"))?;
            let worktree_object = worktree_repo.find_object(oid, None)?;
            worktree_repo
                .checkout_tree(&worktree_object, Some(CheckoutBuilder::new().force()))
                .context("failed to checkout worktree tree")?;
            worktree_repo
                .set_head_detached(oid)
                .context("failed to move worktree HEAD")?;
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            let reference = self.repo.find_reference(&ref_name)?;
            let mut options = git2::WorktreeAddOptions::new();
            options.reference(Some(&reference));
            self.repo
                .worktree(name, path, Some(&options))
                .with_context(|| {
                    format!("failed to create worktree {name} at {}", path.display())
                })?;
        }
        Ok(oid.to_string())
    }

    fn resolve_object(&self, git_ref: &GitRef) -> Result<Object<'_>> {
        let oid = match git_ref {
            GitRef::Commit(commit_sha) => {
                Oid::from_str(commit_sha).with_context(|| format!("bad commit {commit_sha}"))?
            }
            GitRef::Branch(branch_name) => {
                let reference = self
                    .repo
                    .resolve_reference_from_short_name(branch_name)
                    .or_else(|_| {
                        self.repo
                            .find_reference(&format!("refs/remotes/origin/{branch_name}"))
                    })
                    .with_context(|| format!("branch {branch_name} not found"))?;
                reference
                    .target()
                    .ok_or_else(|| anyhow!("branch {branch_name} has no target"))?
            }
            GitRef::Tag(tag_name) => {
                let reference = self
                    .repo
                    .resolve_reference_from_short_name(tag_name)
                    .with_context(|| format!("tag {tag_name} not found"))?;
                reference.peel(git2::ObjectType::Commit)?.id()
            }
        };
        Ok(self.repo.find_object(oid, None)?)
    }
}

fn clone_with_progress(url: &str, dest: &Path) -> Result<Repository> {
    let show = std::io::stderr().is_terminal();

    let mut callbacks = RemoteCallbacks::new();
    if show {
        callbacks.transfer_progress(|stats| {
            print_fetch_progress(&stats);
            true
        });
    }
    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);

    let mut checkout = CheckoutBuilder::new();
    if show {
        checkout.progress(|_, current, total| print_checkout_progress(current, total));
    }

    let repo = RepoBuilder::new()
        .fetch_options(fetch_options)
        .with_checkout(checkout)
        .clone(url, dest)?;
    if show {
        eprintln!();
    }
    Ok(repo)
}

fn print_fetch_progress(stats: &Progress) {
    let total = stats.total_objects();
    if total == 0 {
        return;
    }
    let received = stats.received_objects();
    let percent = received * 100 / total;
    eprint!(
        "\r  fetching: {percent:3}% ({received}/{total} objects, {})   ",
        human_bytes(stats.received_bytes() as u64)
    );
    let _ = std::io::stderr().flush();
}

fn print_checkout_progress(current: usize, total: usize) {
    if total == 0 {
        return;
    }
    let percent = current * 100 / total;
    eprint!("\r  checkout: {percent:3}% ({current}/{total} files)   ");
    let _ = std::io::stderr().flush();
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}
