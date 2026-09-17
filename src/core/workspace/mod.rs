use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub const FORGE_SOURCE_DIR: &str = "ForgeSource";
pub const LAYERS_DIR: &str = "layers";
pub const BUILD_DIR: &str = "build";
pub const STATS_DB_FILE: &str = "stats.db";
pub const GRAPH_DB_FILE: &str = "graph.db";
pub const GITIGNORE_FILE: &str = ".gitignore";

pub struct ProjectPaths {
    pub force_source: PathBuf,
    pub layers: PathBuf,
    pub build: PathBuf,
    pub stats_db: PathBuf,
    pub graph_db: PathBuf,
}

impl ProjectPaths {
    pub fn new(root: &Path) -> Self {
        let force_source = root.join(FORGE_SOURCE_DIR);
        let layers = force_source.join(LAYERS_DIR);
        let stats_db = force_source.join(STATS_DB_FILE);
        let graph_db = force_source.join(GRAPH_DB_FILE);
        let build = root.join(BUILD_DIR);
        Self {
            force_source,
            layers,
            build,
            stats_db,
            graph_db,
        }
    }
}

pub fn ensure_project_layout(root: &Path) -> Result<ProjectPaths> {
    let paths = ProjectPaths::new(root);

    fs::create_dir_all(&paths.force_source)
        .with_context(|| format!("failed to create {}", paths.force_source.display()))?;
    fs::create_dir_all(&paths.layers)
        .with_context(|| format!("failed to create {}", paths.layers.display()))?;
    fs::create_dir_all(&paths.build)
        .with_context(|| format!("failed to create {}", paths.build.display()))?;

    ensure_gitignore(root)?;
    crate::core::store::ensure_database(&paths.stats_db)?;
    crate::core::store::ensure_database(&paths.graph_db)?;
    crate::core::store::init_schema(&paths.stats_db)?;

    Ok(paths)
}

fn ensure_gitignore(root: &Path) -> Result<()> {
    let gitignore_path = root.join(GITIGNORE_FILE);
    if gitignore_path.exists() {
        return Ok(());
    }
    let contents = format!("{FORGE_SOURCE_DIR}/\n{BUILD_DIR}/\n");
    fs::write(&gitignore_path, contents)
        .with_context(|| format!("failed to write {}", gitignore_path.display()))?;
    Ok(())
}
