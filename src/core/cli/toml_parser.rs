use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const CONFIG_FILE_NAME: &str = "BitForge.toml";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BitForgeConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspaceSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<DefaultSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<ServerSection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<LayerReference>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependency: BTreeMap<String, BTreeMap<String, LayerDependency>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled: Vec<String>,

    #[serde(skip)]
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceSection {
    pub name: String,
    #[serde(default = "default_bitbake_version")]
    pub bitbake: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yocto_release: Option<String>,
}

pub fn default_bitbake_version() -> String {
    String::from("2.8")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DefaultSection {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub image: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LayerSection {
    pub name: String,
    pub priority: u32,
    #[serde(default)]
    pub compatible: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildSection {
    pub yocto_release: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSection {
    #[serde(default = "default_server_host")]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default = "default_shutdown_grace_secs")]
    pub shutdown_grace_secs: u64,
    #[serde(default = "default_activity_timeout_mins")]
    pub activity_timeout: u64,
    #[serde(default = "default_true")]
    pub http1_keep_alive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http2_keep_alive_secs: Option<u64>,
}

fn default_server_host() -> String {
    String::from("0.0.0.0")
}

fn default_shutdown_grace_secs() -> u64 {
    20
}

fn default_activity_timeout_mins() -> u64 {
    3
}

fn default_true() -> bool {
    true
}

impl Default for ServerSection {
    fn default() -> Self {
        Self {
            host: default_server_host(),
            port: 0,
            shutdown_grace_secs: default_shutdown_grace_secs(),
            activity_timeout: default_activity_timeout_mins(),
            http1_keep_alive: true,
            http2_keep_alive_secs: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LayerReference {
    pub name: String,
    pub path: String,
    pub priority: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LayerDependency {
    pub giturl: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl LayerDependency {
    pub fn subpath<'a>(&'a self, layer_key: &'a str) -> &'a str {
        self.path.as_deref().unwrap_or(layer_key).trim_matches('/')
    }

    pub fn reference_label(&self) -> String {
        if let Some(commit) = &self.commit {
            format!("commit:{commit}")
        } else if let Some(tag) = &self.tag {
            format!("tag:{tag}")
        } else if let Some(branch) = &self.branch {
            format!("branch:{branch}")
        } else {
            String::from("branch:default")
        }
    }

    pub fn ref_token(&self) -> String {
        if let Some(commit) = &self.commit {
            format!("c-{}", short_sha(commit))
        } else if let Some(tag) = &self.tag {
            format!("t-{}", slug(tag))
        } else if let Some(branch) = &self.branch {
            format!("b-{}", slug(branch))
        } else {
            String::from("default")
        }
    }
}

pub fn slug(value: &str) -> String {
    value
        .chars()
        .map(|character| if character.is_ascii_alphanumeric() { character } else { '-' })
        .collect()
}

fn short_sha(sha: &str) -> String {
    sha.chars().take(12).collect()
}


impl BitForgeConfig {
    pub fn path_in(directory: &Path) -> PathBuf {
        directory.join(CONFIG_FILE_NAME)
    }

    pub fn exists_in(directory: &Path) -> bool {
        Self::path_in(directory).is_file()
    }

    pub fn ensure_present_in(directory: &Path) -> Result<()> {
        if !Self::exists_in(directory) {
            bail!("No BitForge project config found in the directory run `BitForge --init`");
        }
        Ok(())
    }

    pub fn load_required(directory: &Path) -> Result<Self> {
        Self::ensure_present_in(directory)?;
        Self::load_from(directory)
    }

    pub fn load_from(directory: &Path) -> Result<Self> {
        let config_path = Self::path_in(directory);
        let raw_contents = fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        let mut config: BitForgeConfig = toml::from_str(&raw_contents)
            .with_context(|| format!("failed to parse {}", config_path.display()))?;
        config.source_path = config_path;
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let serialized = toml::to_string_pretty(self).context("failed to serialize config")?;
        fs::write(&self.source_path, serialized)
            .with_context(|| format!("failed to write {}", self.source_path.display()))?;
        Ok(())
    }

    pub fn display_name(&self) -> String {
        if let Some(workspace) = &self.workspace {
            return workspace.name.clone();
        }
        if let Some(layer) = &self.layer {
            return layer.name.clone();
        }
        String::from("BitForge")
    }

    pub fn default_image(&self) -> Option<&str> {
        self.default
            .as_ref()
            .map(|section| section.image.as_str())
            .filter(|image| !image.is_empty())
    }

    pub fn effective_release(&self) -> String {
        self.workspace
            .as_ref()
            .and_then(|workspace| workspace.yocto_release.clone())
            .filter(|release| !release.is_empty())
            .unwrap_or_else(|| String::from("scarthgap"))
    }

    pub fn dependency_giturl(&self, gitname: &str) -> Option<String> {
        self.dependency
            .get(gitname)?
            .values()
            .find(|layer| !layer.giturl.is_empty())
            .map(|layer| layer.giturl.clone())
    }

    pub fn validate(&self) -> Result<()> {
        for (gitname, layers) in &self.dependency {
            let mut urls = layers
                .values()
                .map(|layer| layer.giturl.as_str())
                .filter(|url| !url.is_empty());
            if let Some(first) = urls.next() {
                if urls.any(|url| url != first) {
                    bail!(
                        "dependency '{gitname}' maps to multiple git URLs; give one a distinct name"
                    );
                }
            }
        }
        Ok(())
    }

    pub fn server_section(&self) -> ServerSection {
        self.server.clone().unwrap_or_default()
    }

    pub fn is_disabled(&self, name: &str) -> bool {
        self.disabled.iter().any(|entry| entry == name)
    }
}

