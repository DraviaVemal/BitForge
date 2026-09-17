use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::{Result, anyhow, bail};

use super::lock_parser::{LockManager, LockedLayer};
use super::toml_parser::{BitForgeConfig, LayerDependency};
use crate::core::conf::{
    base_ref_token, enabled_layers, resolve_layer_locations, write_bblayers_conf,
};
use crate::core::workspace::{ProjectPaths, ensure_project_layout};
use crate::utils::git_helper::{GitHelper, GitRef};

pub fn run_dependency_add(working_directory: &Path, spec: &str) -> Result<()> {
    let parsed = parse_dependency_spec(spec)?;
    let mut config = BitForgeConfig::load_required(working_directory)?;
    let gitname = allocate_gitname(&config, &parsed.git_url);
    let release = config.effective_release();

    let mut added = Vec::new();
    {
        let entry = config.dependency.entry(gitname.clone()).or_default();
        for parsed_layer in &parsed.layers {
            let (branch, tag, commit) = if parsed_layer.explicit_ref {
                (parsed_layer.branch.clone(), parsed_layer.tag.clone(), parsed_layer.commit.clone())
            } else if parsed.source_branch.is_some()
                || parsed.source_tag.is_some()
                || parsed.source_commit.is_some()
            {
                (parsed.source_branch.clone(), parsed.source_tag.clone(), parsed.source_commit.clone())
            } else {
                (Some(release.clone()), None, None)
            };
            let mut key = parsed_layer.name.clone();
            if let Some(existing) = entry.get(&key) {
                if existing.path.as_deref() != parsed_layer.path.as_deref() {
                    key = parsed_layer.path.clone().unwrap_or_else(|| parsed_layer.name.clone());
                }
            }
            entry.insert(
                key.clone(),
                LayerDependency {
                    giturl: parsed.git_url.clone(),
                    branch,
                    tag,
                    commit,
                    path: parsed_layer.path.clone(),
                },
            );
            added.push(key);
        }
    }
    config.validate()?;
    config.save()?;

    install_all(working_directory, &config)?;
    guard_duplicate_collections(working_directory, &mut config, &gitname, &added)?;

    config.save()?;
    write_bblayers_conf(working_directory, &config)?;

    log::info!("Added {} layer(s) under '{gitname}': {}", added.len(), added.join(", "));
    Ok(())
}

pub(crate) fn install_all(working_directory: &Path, config: &BitForgeConfig) -> Result<()> {
    let paths = ensure_project_layout(working_directory)?;
    let mut lock = LockManager::load(working_directory)?;
    let release = config.effective_release();

    lock.data.layers.retain(|gitname, _| config.dependency.contains_key(gitname));

    for (gitname, layers) in &config.dependency {
        let giturl = layers
            .values()
            .map(|layer| layer.giturl.clone())
            .find(|url| !url.is_empty())
            .unwrap_or_default();
        if giturl.is_empty() {
            continue;
        }
        let base_dir = paths.layers.join(gitname);
        let helper = GitHelper::clone_or_open(&giturl, &base_dir)?;
        helper.fetch().ok();

        let base_token = base_ref_token(layers, &release);
        let base_commit = match layers.values().find(|layer| layer.ref_token() == base_token) {
            Some(base_layer) => {
                if let Some(git_ref) = layer_git_ref(base_layer) {
                    helper.checkout(&git_ref)?;
                }
                helper.head_commit()?
            }
            None => helper.head_commit()?,
        };

        let locations = resolve_layer_locations(gitname, layers, &release);
        let lock_layers = lock.data.layers.entry(gitname.clone()).or_default();
        lock_layers.retain(|layername, _| layers.contains_key(layername));

        for (layername, layer) in layers {
            let token = layer.ref_token();
            let worktree_dir = locations
                .get(layername)
                .map(|(dir, _)| dir.clone())
                .unwrap_or_default();
            let commit = if token == base_token {
                base_commit.clone()
            } else {
                let worktree_path = base_dir.join(".forge").join(&token);
                let git_ref = layer_git_ref(layer)
                    .ok_or_else(|| anyhow!("layer '{gitname}.{layername}' has no ref for its worktree"))?;
                helper.ensure_worktree(&token, &worktree_path, &git_ref)?
            };
            lock_layers.insert(
                layername.clone(),
                LockedLayer {
                    giturl: giturl.clone(),
                    reference: layer.reference_label(),
                    commit,
                    worktree: worktree_dir,
                },
            );
        }
    }
    lock.flush()?;
    Ok(())
}

fn layer_git_ref(layer: &LayerDependency) -> Option<GitRef> {
    if let Some(commit) = &layer.commit {
        Some(GitRef::Commit(commit.clone()))
    } else if let Some(tag) = &layer.tag {
        Some(GitRef::Tag(tag.clone()))
    } else {
        layer.branch.clone().map(GitRef::Branch)
    }
}

fn allocate_gitname(config: &BitForgeConfig, git_url: &str) -> String {
    let base = derive_dependency_name_from_url(git_url);
    let free_or_same = |candidate: &str| match config.dependency_giturl(candidate) {
        None => true,
        Some(existing) => existing == git_url,
    };
    if free_or_same(&base) {
        return base;
    }
    for candidate in alias_candidates(git_url, &base) {
        if free_or_same(&candidate) {
            return candidate;
        }
    }
    for suffix in 2.. {
        let candidate = format!("{base}-{suffix}");
        if free_or_same(&candidate) {
            return candidate;
        }
    }
    base
}

fn alias_candidates(git_url: &str, base: &str) -> Vec<String> {
    let cleaned = git_url.trim_end_matches('/').trim_end_matches(".git");
    let segments: Vec<&str> = cleaned.split(['/', ':']).filter(|part| !part.is_empty()).collect();
    let mut candidates = Vec::new();
    if segments.len() >= 2 {
        let owner = alias_slug(segments[segments.len() - 2]);
        candidates.push(format!("{owner}-{base}"));
    }
    if segments.len() >= 3 {
        let host = alias_slug(segments[segments.len() - 3]);
        let owner = alias_slug(segments[segments.len() - 2]);
        candidates.push(format!("{host}-{owner}-{base}"));
    }
    candidates
}

fn alias_slug(value: &str) -> String {
    value
        .chars()
        .map(|character| if character.is_ascii_alphanumeric() { character } else { '-' })
        .collect()
}

fn read_collection(root: &Path, layer_path: &str) -> Option<String> {
    let conf = root.join(layer_path).join("conf").join("layer.conf");
    let text = fs::read_to_string(conf).ok()?;
    for line in text.lines() {
        if let Some((_, rest)) = line.split_once("BBFILE_COLLECTIONS") {
            let start = rest.find('"')?;
            let remainder = &rest[start + 1..];
            let end = remainder.find('"')?;
            return remainder[..end].split_whitespace().next().map(str::to_string);
        }
    }
    None
}

fn guard_duplicate_collections(
    root: &Path,
    config: &mut BitForgeConfig,
    gitname: &str,
    added: &[String],
) -> Result<()> {
    let release = config.effective_release();
    let locations = match config.dependency.get(gitname) {
        Some(layers) => resolve_layer_locations(gitname, layers, &release),
        None => return Ok(()),
    };
    let new_paths: HashSet<String> = added
        .iter()
        .filter_map(|name| locations.get(name).map(|(_, path)| path.clone()))
        .collect();

    let mut provided: HashMap<String, String> = HashMap::new();
    for layer in enabled_layers(config) {
        if !layer.linked || new_paths.contains(&layer.path) {
            continue;
        }
        if let Some(collection) = read_collection(root, &layer.path) {
            provided.entry(collection).or_insert(layer.path.clone());
        }
    }

    let mut conflicts = Vec::new();
    for name in added {
        let Some((_, path)) = locations.get(name) else {
            continue;
        };
        if let Some(collection) = read_collection(root, path) {
            match provided.get(&collection) {
                Some(existing) => conflicts.push((path.clone(), collection, existing.clone())),
                None => {
                    provided.insert(collection, path.clone());
                }
            }
        }
    }

    for (path, collection, existing) in conflicts {
        if !config.disabled.contains(&path) {
            config.disabled.push(path.clone());
        }
        log::warn!(
            "'{path}' provides collection '{collection}' already provided by '{existing}'; kept on disk but delinked to avoid a BitBake duplicate-collection error."
        );
    }
    Ok(())
}

pub fn run_dependency_delink(working_directory: &Path, layer_path: &str) -> Result<()> {
    let mut config = BitForgeConfig::load_required(working_directory)?;
    let known = crate::core::conf::enabled_layers(&config)
        .into_iter()
        .any(|layer| layer.path == layer_path && layer.dependency.is_some());
    if !known {
        bail!("'{layer_path}' is not a dependency layer");
    }
    if !config.disabled.iter().any(|entry| entry == layer_path) {
        config.disabled.push(layer_path.to_string());
    }
    config.save()?;
    write_bblayers_conf(working_directory, &config)?;
    let detail = format!("delinked layer '{layer_path}'; skipped in auto-generated bblayers.conf");
    log::info!(target: "delink", "{detail}");
    crate::utils::logging::append_project_log(working_directory, "delink", "INFO", &detail);
    log::info!("Delinked layer '{layer_path}' (kept on disk, removed from bblayers.conf)");
    Ok(())
}

pub fn run_dependency_relink(working_directory: &Path, layer_path: &str) -> Result<()> {
    let mut config = BitForgeConfig::load_required(working_directory)?;
    config.disabled.retain(|entry| entry != layer_path);
    config.save()?;
    write_bblayers_conf(working_directory, &config)?;
    let detail = format!("relinked layer '{layer_path}'; restored to auto-generated bblayers.conf");
    log::info!(target: "delink", "{detail}");
    crate::utils::logging::append_project_log(working_directory, "delink", "INFO", &detail);
    log::info!("Relinked layer '{layer_path}'");
    Ok(())
}

pub fn run_dependency_remove(working_directory: &Path, dependency_name: &str) -> Result<()> {
    let mut config = BitForgeConfig::load_required(working_directory)?;
    if config.dependency.remove(dependency_name).is_none() {
        bail!("'{dependency_name}' is not a tracked dependency");
    }
    let paths = ProjectPaths::new(working_directory);
    let dependency_directory = paths.layers.join(dependency_name);
    let disabled_prefix = format!("ForgeSource/layers/{dependency_name}");
    config
        .disabled
        .retain(|entry| entry != dependency_name && !entry.starts_with(&disabled_prefix));
    config.save()?;

    let mut lock = LockManager::load(working_directory)?;
    lock.data.layers.remove(dependency_name);
    lock.flush()?;

    if dependency_directory.exists() {
        fs::remove_dir_all(&dependency_directory)?;
    }

    write_bblayers_conf(working_directory, &config)?;

    log::info!("Removed dependency '{dependency_name}'");
    Ok(())
}

struct ParsedLayer {
    name: String,
    path: Option<String>,
    branch: Option<String>,
    tag: Option<String>,
    commit: Option<String>,
    explicit_ref: bool,
}

struct ParsedDependencySpec {
    git_url: String,
    source_branch: Option<String>,
    source_tag: Option<String>,
    source_commit: Option<String>,
    layers: Vec<ParsedLayer>,
}

fn parse_dependency_spec(spec: &str) -> Result<ParsedDependencySpec> {
    let (main_part, sub_part) = match spec.split_once('#') {
        Some((main, sub)) => (main.to_string(), Some(sub.to_string())),
        None => (spec.to_string(), None),
    };

    let (git_url, source_ref) = match main_part.rsplit_once('@') {
        Some((url, reference)) if is_ref_spec(reference) => {
            (url.to_string(), Some(reference.to_string()))
        }
        _ => (main_part.clone(), None),
    };
    if git_url.is_empty() {
        bail!("missing git url in dependency spec '{spec}'");
    }

    let (mut source_branch, mut source_tag, mut source_commit) = (None, None, None);
    if let Some(reference) = &source_ref {
        apply_ref(reference, &mut source_branch, &mut source_tag, &mut source_commit)?;
    }

    let gitname = derive_dependency_name_from_url(&git_url);
    let mut layers = Vec::new();
    match sub_part {
        Some(sub) if !sub.trim().is_empty() => {
            for raw_entry in sub.split(',') {
                let entry = raw_entry.trim();
                if entry.is_empty() {
                    continue;
                }
                let (name_part, entry_ref) = match entry.rsplit_once('@') {
                    Some((path, reference)) if is_ref_spec(reference) => {
                        (path.to_string(), Some(reference.to_string()))
                    }
                    _ => (entry.to_string(), None),
                };
                let name = name_part.trim_matches('/').to_string();
                if name.is_empty() {
                    continue;
                }
                let (mut branch, mut tag, mut commit) = (None, None, None);
                let explicit_ref = entry_ref.is_some();
                if let Some(reference) = &entry_ref {
                    apply_ref(reference, &mut branch, &mut tag, &mut commit)?;
                }
                let (name, path) = match name.rsplit_once('/') {
                    Some((_, last)) if !last.is_empty() => (last.to_string(), Some(name.clone())),
                    _ => (name, None),
                };
                layers.push(ParsedLayer { name, path, branch, tag, commit, explicit_ref });
            }
        }
        _ => {
            layers.push(ParsedLayer {
                name: gitname,
                path: None,
                branch: None,
                tag: None,
                commit: None,
                explicit_ref: false,
            });
        }
    }

    if layers.is_empty() {
        bail!("no layers resolved from dependency spec '{spec}'");
    }

    Ok(ParsedDependencySpec {
        git_url,
        source_branch,
        source_tag,
        source_commit,
        layers,
    })
}

fn apply_ref(
    reference: &str,
    branch: &mut Option<String>,
    tag: &mut Option<String>,
    commit: &mut Option<String>,
) -> Result<()> {
    let (kind, value) = reference
        .split_once(':')
        .ok_or_else(|| anyhow!("expected `@b|c|t:value` in ref '{reference}'"))?;
    if value.is_empty() {
        bail!("missing ref value in '{reference}'");
    }
    match kind {
        "b" => *branch = Some(value.to_string()),
        "c" => *commit = Some(value.to_string()),
        "t" => *tag = Some(value.to_string()),
        other => bail!("unknown ref kind '{other}', expected b (branch), c (commit) or t (tag)"),
    }
    Ok(())
}

fn is_ref_spec(reference: &str) -> bool {
    matches!(
        reference.split_once(':'),
        Some((kind, value)) if !value.is_empty() && matches!(kind, "b" | "c" | "t")
    )
}

fn derive_dependency_name_from_url(git_url: &str) -> String {
    let trimmed = git_url.trim_end_matches('/');
    let last_segment = trimmed.rsplit(['/', ':']).next().unwrap_or(trimmed);
    last_segment.trim_end_matches(".git").to_string()
}
