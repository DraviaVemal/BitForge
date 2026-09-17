use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const LOCK_FILE_NAME: &str = "BitForge.lock";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LockData {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub layers: BTreeMap<String, BTreeMap<String, LockedLayer>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LockedLayer {
    #[serde(default)]
    pub giturl: String,
    #[serde(default, rename = "ref", skip_serializing_if = "String::is_empty")]
    pub reference: String,
    #[serde(default)]
    pub commit: String,
    #[serde(default)]
    pub worktree: String,
}

pub struct LockManager {
    lock_file_path: PathBuf,
    pub data: LockData,
}

impl LockManager {
    pub fn load(directory: &Path) -> Result<Self> {
        let lock_file_path = directory.join(LOCK_FILE_NAME);

        let data = if lock_file_path.exists() {
            let raw_contents = fs::read_to_string(&lock_file_path)
                .with_context(|| format!("failed to read {}", lock_file_path.display()))?;

            if raw_contents.trim().is_empty() {
                LockData {
                    version: 1,
                    ..Default::default()
                }
            } else {
                toml::from_str(&raw_contents)
                    .with_context(|| format!("failed to parse {}", lock_file_path.display()))?
            }
        } else {
            LockData {
                version: 1,
                ..Default::default()
            }
        };

        Ok(Self {
            lock_file_path,
            data,
        })
    }

    pub fn flush(&self) -> Result<()> {
        let serialized =
            toml::to_string_pretty(&self.data).context("failed to serialize lock data")?;
        fs::write(&self.lock_file_path, serialized)
            .with_context(|| format!("failed to write {}", self.lock_file_path.display()))?;
        Ok(())
    }
}
