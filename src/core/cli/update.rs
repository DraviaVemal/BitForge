use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

const DEFAULT_REPO: &str = "DraviaVemal/BitForge";

pub const VERSION: &str = env!("BITFORGE_VERSION");
const SEMVER: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    published_at: String,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

pub fn run_update(beta: bool) -> Result<()> {
    if Command::new("curl").arg("--version").output().is_err() {
        bail!("curl is required for --update");
    }

    let repo = std::env::var("BITFORGE_REPO").unwrap_or_else(|_| DEFAULT_REPO.to_string());
    let releases = fetch_releases(&repo)?;
    let release = choose_release(&releases, beta)
        .ok_or_else(|| anyhow!("no suitable release found for {repo}"))?;

    let latest = release.tag_name.trim_start_matches('v');
    if !is_newer(latest, SEMVER) {
        log::info!("BitForge is already up to date ({VERSION})");
        return Ok(());
    }

    let asset_name = asset_name();
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .ok_or_else(|| anyhow!("release {} has no asset '{asset_name}'", release.tag_name))?;

    let current_exe = std::env::current_exe().context("cannot resolve current executable")?;
    let staging = current_exe.with_file_name(".bitforge-update.tmp");

    log::info!("Downloading BitForge {latest}...");
    download(&asset.browser_download_url, &staging)?;
    make_executable(&staging)?;
    fs::rename(&staging, &current_exe)
        .with_context(|| format!("failed to replace {}", current_exe.display()))?;

    log::info!("Updated BitForge {SEMVER} -> {latest}");
    Ok(())
}

fn fetch_releases(repo: &str) -> Result<Vec<Release>> {
    let url = format!("https://api.github.com/repos/{repo}/releases");
    let output = Command::new("curl")
        .args([
            "-fsSL",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: BitForge",
            &url,
        ])
        .output()
        .context("failed to run curl")?;
    if !output.status.success() {
        bail!("failed to fetch releases from {url}");
    }
    serde_json::from_slice(&output.stdout).context("failed to parse releases response")
}

fn choose_release(releases: &[Release], beta: bool) -> Option<&Release> {
    let stable = releases
        .iter()
        .filter(|release| !release.draft && !release.prerelease)
        .max_by(|left, right| left.published_at.cmp(&right.published_at));

    if !beta {
        return stable;
    }

    let prerelease = releases
        .iter()
        .filter(|release| !release.draft && release.prerelease)
        .max_by(|left, right| left.published_at.cmp(&right.published_at));

    match (stable, prerelease) {
        (Some(stable), Some(pre)) => {
            if pre.published_at > stable.published_at {
                Some(pre)
            } else {
                Some(stable)
            }
        }
        (None, pre) => pre,
        (stable, None) => stable,
    }
}

fn is_newer(candidate: &str, current: &str) -> bool {
    version_key(candidate) > version_key(current)
}

fn version_key(version: &str) -> Vec<u64> {
    version
        .trim_start_matches('v')
        .split(|character: char| character == '.' || character == '+' || character == '-')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

fn asset_name() -> String {
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "unknown"
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "unknown"
    };
    format!("bitforge-{os}-{arch}")
}

fn download(url: &str, destination: &Path) -> Result<()> {
    let status = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(destination)
        .arg(url)
        .status()
        .context("failed to run curl")?;
    if !status.success() {
        bail!("download failed for {url}");
    }
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .with_context(|| format!("failed to set permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}
