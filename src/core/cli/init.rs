use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use log::{info, warn};

use super::dependency::install_all;
use super::toml_parser::{
    BitForgeConfig, CONFIG_FILE_NAME, LayerReference, LayerSection,
};
use crate::core::workspace::FORGE_SOURCE_DIR;
use crate::utils::git_helper::{GitHelper, GitRef};

pub(crate) const DEFAULT_YOCTO_RELEASE: &str = "scarthgap";
pub(crate) const DEFAULT_LAYER_PRIORITY: u32 = 6;
pub(crate) const LAYERS_DIRECTORY: &str = "layers";

const POKY_GIT_URL: &str = "https://git.yoctoproject.org/poky";
const POKY_DEPENDENCY_NAME: &str = "poky";
const POKY_DISTRO: &str = "poky";

const OE_CORE_GIT_URL: &str = "https://git.openembedded.org/openembedded-core";
const OE_CORE_DEPENDENCY_NAME: &str = "openembedded-core";

const BITBAKE_GIT_URL: &str = "https://git.openembedded.org/bitbake";
pub(crate) const BITBAKE_VERSION: &str = "2.8";
const BITBAKE_DIR: &str = "bitbake";

const OE_CORE_TEMPLATE: &str = include_str!("templates/bitforge.oe-core.toml");
const POKY_TEMPLATE: &str = include_str!("templates/bitforge.poky.toml");

pub fn run_init(
    base_directory: &Path,
    dir_name: Option<&str>,
    force: bool,
    poky_version: Option<&str>,
) -> Result<()> {
    if let Some(version) = poky_version {
        if version.is_empty() {
            bail!("`--poky` requires a version, e.g. --poky scarthgap");
        }
    }

    let target_directory = match dir_name {
        Some(name) => base_directory.join(name),
        None => base_directory.to_path_buf(),
    };

    let created_target = !target_directory.exists();
    if created_target {
        fs::create_dir_all(&target_directory)
            .with_context(|| format!("failed to create {}", target_directory.display()))?;
    }

    if directory_has_entries(&target_directory)? {
        if force {
            warn!(
                "{} is not empty; initializing anyway (--force)",
                target_directory.display()
            );
        } else {
            bail!(
                "{} is not empty; use --force to initialize anyway",
                target_directory.display()
            );
        }
    }

    match initialize_workspace(&target_directory, poky_version) {
        Ok(()) => Ok(()),
        Err(error) => {
            cleanup_partial_init(&target_directory, created_target);
            Err(error)
        }
    }
}

fn initialize_workspace(target_directory: &Path, poky_version: Option<&str>) -> Result<()> {
    let project_slug = derive_project_slug(target_directory);
    let root_config_path = target_directory.join(CONFIG_FILE_NAME);

    if root_config_path.exists() {
        bail!("{} already exists", root_config_path.display());
    }

    crate::core::tracking::ensure_git_project(target_directory)?;

    let (base_name, base_git_url, release, is_poky, template): (&str, &str, &str, bool, &str) =
        match poky_version {
            Some(version) => (POKY_DEPENDENCY_NAME, POKY_GIT_URL, version, true, POKY_TEMPLATE),
            None => (
                OE_CORE_DEPENDENCY_NAME,
                OE_CORE_GIT_URL,
                DEFAULT_YOCTO_RELEASE,
                false,
                OE_CORE_TEMPLATE,
            ),
        };

    let starter_layer_name = derive_layer_name(&project_slug);
    let layer_path = format!("{LAYERS_DIRECTORY}/{starter_layer_name}");
    scaffold_own_layer(target_directory, &starter_layer_name, release)?;

    let rendered = template
        .replace("{{PROJECT_NAME}}", &project_slug)
        .replace("{{RELEASE}}", release)
        .replace("{{LAYER_NAME}}", &starter_layer_name)
        .replace("{{LAYER_PATH}}", &layer_path);
    fs::write(&root_config_path, rendered)
        .with_context(|| format!("failed to write {}", root_config_path.display()))?;

    let root_config = BitForgeConfig::load_from(target_directory)?;

    crate::core::workspace::ensure_project_layout(target_directory)?;
    crate::core::tracking::record_revision(target_directory, "init").ok();
    ensure_bitbake(target_directory, BITBAKE_VERSION)?;

    if !GitHelper::remote_branch_exists(base_git_url, release)? {
        bail!("release '{release}' not found at {base_git_url}");
    }
    info!("Cloning {base_name} ({release}) into ForgeSource; this may take a while...");
    install_all(target_directory, &root_config)?;

    crate::core::conf::ensure_build_conf(target_directory, &root_config)?;
    if is_poky {
        crate::core::conf::set_local_conf_value(target_directory, "DISTRO", POKY_DISTRO)?;
    }

    let flavor = if is_poky { "poky" } else { "openembedded-core" };
    info!(
        "Initialized BitForge {flavor} workspace '{project_slug}' with layer '{starter_layer_name}'"
    );
    Ok(())
}

pub fn ensure_bitbake(root: &Path, version: &str) -> Result<PathBuf> {
    let destination = root.join(FORGE_SOURCE_DIR).join(BITBAKE_DIR);
    if destination.join(".git").exists() {
        return Ok(destination);
    }

    let version = if version.is_empty() {
        BITBAKE_VERSION
    } else {
        version
    };

    info!("Cloning bitbake ({version}) into {FORGE_SOURCE_DIR}/{BITBAKE_DIR}...");
    let git_helper = GitHelper::clone_or_open(BITBAKE_GIT_URL, &destination)
        .context("failed to clone bitbake")?;
    git_helper.fetch().ok();
    git_helper
        .checkout(&GitRef::Branch(version.to_string()))
        .with_context(|| format!("bitbake version '{version}' not found"))?;
    Ok(destination)
}

fn cleanup_partial_init(target_directory: &Path, created_target: bool) {
    if created_target {
        let _ = fs::remove_dir_all(target_directory);
        return;
    }
    for generated in [
        CONFIG_FILE_NAME,
        "BitForge.lock",
        ".gitignore",
        "conf",
        "build",
        "ForgeSource",
        LAYERS_DIRECTORY,
    ] {
        let path = target_directory.join(generated);
        let _ = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
    }
}

fn directory_has_entries(directory: &Path) -> Result<bool> {
    let mut entries = fs::read_dir(directory)
        .with_context(|| format!("failed to read {}", directory.display()))?;
    Ok(entries.next().is_some())
}

pub(crate) fn scaffold_own_layer(
    working_directory: &Path,
    layer_name: &str,
    compatible_release: &str,
) -> Result<LayerReference> {
    let layer_directory = working_directory.join(LAYERS_DIRECTORY).join(layer_name);
    fs::create_dir_all(&layer_directory)
        .with_context(|| format!("failed to create {}", layer_directory.display()))?;

    let layer_config = BitForgeConfig {
        layer: Some(LayerSection {
            name: layer_name.to_string(),
            priority: DEFAULT_LAYER_PRIORITY,
            compatible: vec![compatible_release.to_string()],
        }),
        source_path: layer_directory.join(CONFIG_FILE_NAME),
        ..Default::default()
    };
    layer_config.save()?;

    crate::core::conf::write_layer_conf(
        &layer_directory,
        layer_name,
        DEFAULT_LAYER_PRIORITY,
        compatible_release,
    )?;

    write_placeholder_recipe(&layer_directory)?;

    Ok(LayerReference {
        name: layer_name.to_string(),
        path: format!("{LAYERS_DIRECTORY}/{layer_name}"),
        priority: DEFAULT_LAYER_PRIORITY,
    })
}

fn write_placeholder_recipe(layer_directory: &Path) -> Result<()> {
    let recipe_directory = layer_directory
        .join("recipes-example")
        .join("bitforge-example");
    fs::create_dir_all(&recipe_directory)
        .with_context(|| format!("failed to create {}", recipe_directory.display()))?;
    let recipe_path = recipe_directory.join("bitforge-example_0.1.bb");
    let contents = "SUMMARY = \"BitForge starter placeholder recipe\"\n\
DESCRIPTION = \"Keeps this layer non-empty so BitBake does not warn about an unused \
BBFILE_PATTERN. Replace it with your own recipes under recipes-*/.\"\n\
LICENSE = \"CLOSED\"\n\
\n\
do_compile[noexec] = \"1\"\n\
do_install[noexec] = \"1\"\n";
    fs::write(&recipe_path, contents)
        .with_context(|| format!("failed to write {}", recipe_path.display()))?;
    Ok(())
}

fn derive_project_slug(working_directory: &Path) -> String {
    let directory_name = working_directory
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    sanitize_slug(directory_name)
}

pub(crate) fn sanitize_slug(value: &str) -> String {
    let mut slug = String::new();
    for character in value.to_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if matches!(character, ' ' | '_' | '-') && !slug.ends_with('-') {
            slug.push('-');
        }
    }

    let trimmed_slug = slug.trim_matches('-').to_string();
    if trimmed_slug.is_empty() {
        String::from("project")
    } else {
        trimmed_slug
    }
}

pub(crate) fn derive_layer_name(project_slug: &str) -> String {
    if project_slug.starts_with("meta-") {
        project_slug.to_string()
    } else {
        format!("meta-{project_slug}")
    }
}
