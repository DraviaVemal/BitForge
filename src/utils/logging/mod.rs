mod init;

pub use init::*;

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use crate::core::workspace::FORGE_SOURCE_DIR;

pub const PROJECT_LOG_FILE: &str = "bitforge.log";

pub fn append_project_log(root: &Path, source: &str, level: &str, message: &str) {
    let path = root.join(FORGE_SOURCE_DIR).join(PROJECT_LOG_FILE);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(
            file,
            "| {} | {:<5} | {} | {}",
            init::format_timestamp(),
            level,
            source,
            message
        );
    }
}