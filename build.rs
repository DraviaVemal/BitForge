use std::path::Path;
use std::process::Command;

fn main() {
    let version = emit_version();
    build_web(&version);
}

fn emit_version() -> String {
    let package_version =
        std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| String::from("0.0.0"));
    let version = match git_short_hash() {
        Some(hash) => format!("{package_version}+{hash}"),
        None => package_version,
    };
    println!("cargo:rustc-env=BITFORGE_VERSION={version}");
    println!("cargo:rerun-if-changed=.git/HEAD");
    version
}

fn git_short_hash() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if hash.is_empty() { None } else { Some(hash) }
}

fn build_web(version: &str) {
    let manifest_directory = env!("CARGO_MANIFEST_DIR");
    let web_directory = Path::new(manifest_directory).join("src").join("web");
    let dist_directory = web_directory.join("dist");

    for watched_entry in ["src", "index.html", "package.json", "vite.config.ts", "tsconfig.json"] {
        println!(
            "cargo:rerun-if-changed={}",
            web_directory.join(watched_entry).display()
        );
    }

    if !web_directory.join("package.json").exists() {
        println!("cargo:warning=BitForge: src/web has no package.json; skipping web build");
        ensure_fallback_dist(&dist_directory);
        return;
    }

    let npm_program = match npm_command() {
        Some(program) => program,
        None => {
            println!("cargo:warning=BitForge: npm not found; embedding fallback web page");
            ensure_fallback_dist(&dist_directory);
            return;
        }
    };

    if !web_directory.join("node_modules").exists() {
        run(&npm_program, &["install"], &web_directory, "npm install");
    }
    run_with_env(
        &npm_program,
        &["run", "build"],
        &web_directory,
        "npm run build",
        &[("VITE_BITFORGE_VERSION", version)],
    );

    if !dist_directory.join("index.html").exists() {
        ensure_fallback_dist(&dist_directory);
    }
}

fn npm_command() -> Option<String> {
    let candidates = if cfg!(windows) {
        vec!["npm.cmd", "npm"]
    } else {
        vec!["npm"]
    };
    for candidate in candidates {
        if Command::new(candidate)
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
        {
            return Some(candidate.to_string());
        }
    }
    None
}

fn run(program: &str, arguments: &[&str], working_directory: &Path, label: &str) {
    run_with_env(program, arguments, working_directory, label, &[]);
}

fn run_with_env(
    program: &str,
    arguments: &[&str],
    working_directory: &Path,
    label: &str,
    env: &[(&str, &str)],
) {
    let mut command = Command::new(program);
    command.args(arguments).current_dir(working_directory);
    for (key, value) in env {
        command.env(key, value);
    }
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("BitForge: failed to spawn `{label}`: {error}"));
    if !status.success() {
        panic!("BitForge: `{label}` failed with status {status}");
    }
}

fn ensure_fallback_dist(dist_directory: &Path) {
    if dist_directory.join("index.html").exists() {
        return;
    }
    std::fs::create_dir_all(dist_directory).expect("BitForge: cannot create web dist dir");
    let fallback_html = include_str!("assets/fallback-index.html");
    std::fs::write(dist_directory.join("index.html"), fallback_html)
        .expect("BitForge: cannot write fallback index.html");
}