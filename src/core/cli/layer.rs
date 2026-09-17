use std::fs;
use std::path::Path;

use anyhow::{Result, bail};

use super::init::{
    LAYERS_DIRECTORY, derive_layer_name, sanitize_slug, scaffold_own_layer,
};
use super::toml_parser::BitForgeConfig;
use crate::core::conf::ensure_build_conf;

pub fn run_add_layer(working_directory: &Path, raw_name: &str) -> Result<()> {
    let layer_name = derive_layer_name(&sanitize_slug(raw_name));

    let mut config = BitForgeConfig::load_required(working_directory)?;
    if config.layers.iter().any(|layer| layer.name == layer_name) {
        bail!("layer '{layer_name}' already exists");
    }

    let layer_directory = working_directory.join(LAYERS_DIRECTORY).join(&layer_name);
    if layer_directory.exists() {
        bail!("{} already exists", layer_directory.display());
    }

    let compatible_release = config.effective_release();

    let layer_reference = scaffold_own_layer(working_directory, &layer_name, &compatible_release)?;
    config.layers.push(layer_reference);
    config.save()?;

    ensure_build_conf(working_directory, &config)?;

    log::info!("Added layer '{layer_name}'");
    Ok(())
}

pub fn run_remove_layer(working_directory: &Path, raw_name: &str) -> Result<()> {
    let candidate = derive_layer_name(&sanitize_slug(raw_name));

    let mut config = BitForgeConfig::load_required(working_directory)?;
    let position = config
        .layers
        .iter()
        .position(|layer| layer.name == raw_name || layer.name == candidate);

    let Some(position) = position else {
        bail!("layer '{raw_name}' is not a workspace layer");
    };

    let removed = config.layers.remove(position);
    config.save()?;

    let layer_directory = working_directory.join(LAYERS_DIRECTORY).join(&removed.name);
    if layer_directory.exists() {
        fs::remove_dir_all(&layer_directory)?;
    }

    ensure_build_conf(working_directory, &config)?;

    log::info!("Removed layer '{}'", removed.name);
    Ok(())
}
