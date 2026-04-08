//! Build script: download Javy CLI into OUT_DIR and emit the QuickJS plugin.
//!
//! - Downloads the platform-specific binary from GitHub releases (cached).
//! - Runs `javy emit-plugin` to produce the QuickJS engine plugin WASM (cached).
//! - Sets `JAVY_CLI_PATH` and `JAVY_PLUGIN_PATH` compile-time env vars.
//! - Copies the binary and plugin to `target/` for easy Docker COPY.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const JAVY_VERSION: &str = "8.1.0";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // Tracks branch switches but not new commits on the same branch during
    // local incremental builds. This is acceptable — CI always does a clean
    // build where build.rs runs unconditionally.
    println!("cargo:rerun-if-changed=../.git/HEAD");

    // Capture short commit hash at build time
    let commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=GIT_COMMIT_SHORT={commit}");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let javy_bin = out_dir.join("javy");

    download_javy(&javy_bin);
    emit_plugin(&javy_bin, &out_dir);

    // Expose paths as compile-time env vars for the binary
    println!("cargo:rustc-env=JAVY_CLI_PATH={}", javy_bin.display());
    println!(
        "cargo:rustc-env=JAVY_PLUGIN_PATH={}",
        out_dir.join("engine.wasm").display()
    );

    // Also copy to a stable location under target/ for Docker builds.
    // OUT_DIR is something like target/{profile}/build/proxy-{hash}/out/
    // Walk up to find the target dir (parent of "build" directory).
    if let Some(target_dir) = find_target_dir(&out_dir) {
        let stable_javy = target_dir.join("javy");
        if !stable_javy.exists()
            || fs::metadata(&stable_javy).map(|m| m.len()).unwrap_or(0)
                != fs::metadata(&javy_bin).map(|m| m.len()).unwrap_or(1)
        {
            let _ = fs::copy(&javy_bin, &stable_javy);
        }

        let plugin_path = out_dir.join("engine.wasm");
        let stable_plugin = target_dir.join("engine.wasm");
        if !stable_plugin.exists()
            || fs::metadata(&stable_plugin).map(|m| m.len()).unwrap_or(0)
                != fs::metadata(&plugin_path).map(|m| m.len()).unwrap_or(1)
        {
            let _ = fs::copy(&plugin_path, &stable_plugin);
        }
    }
}

fn download_javy(javy_bin: &PathBuf) {
    if javy_bin.exists() {
        return;
    }

    let (os, arch) = (env::consts::OS, env::consts::ARCH);
    let asset_name = match (os, arch) {
        ("linux", "x86_64") => format!("javy-x86_64-linux-v{JAVY_VERSION}.gz"),
        ("linux", "aarch64") => format!("javy-arm-linux-v{JAVY_VERSION}.gz"),
        ("macos", "aarch64") => format!("javy-arm-macos-v{JAVY_VERSION}.gz"),
        ("macos", "x86_64") => format!("javy-x86_64-macos-v{JAVY_VERSION}.gz"),
        _ => panic!("Unsupported platform: {os}/{arch}"),
    };

    let url = format!(
        "https://github.com/bytecodealliance/javy/releases/download/v{JAVY_VERSION}/{asset_name}"
    );

    let gz_path = javy_bin.with_extension("gz");

    eprintln!("Downloading Javy CLI v{JAVY_VERSION} from {url}");

    let status = Command::new("curl")
        .args(["-LSs", "-o"])
        .arg(&gz_path)
        .arg(&url)
        .status()
        .expect("Failed to run curl — is it installed?");
    assert!(status.success(), "curl failed to download {url}");

    let status = Command::new("gzip")
        .args(["-d", "-f"])
        .arg(&gz_path)
        .status()
        .expect("Failed to run gzip");
    assert!(status.success(), "gzip decompression failed");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(javy_bin, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// Emit the Javy QuickJS engine plugin WASM (`engine.wasm`).
/// The plugin is the precompiled QuickJS engine (~869 KB) that is instantiated once
/// at server startup. Per-function bytecode modules (1-16 KB) import from it.
fn emit_plugin(javy_bin: &std::path::Path, out_dir: &std::path::Path) {
    let plugin_path = out_dir.join("engine.wasm");
    if plugin_path.exists() {
        return;
    }

    eprintln!("Emitting Javy QuickJS plugin to {}", plugin_path.display());

    let status = Command::new(javy_bin)
        .args(["emit-plugin", "-o"])
        .arg(&plugin_path)
        .status()
        .expect("Failed to run javy emit-plugin");
    assert!(status.success(), "javy emit-plugin failed");
}

/// Walk up from OUT_DIR to find the `target/{profile}` directory.
/// OUT_DIR is typically `target/{profile}/build/{crate}-{hash}/out`.
fn find_target_dir(out_dir: &std::path::Path) -> Option<PathBuf> {
    let mut dir = out_dir;
    while let Some(parent) = dir.parent() {
        if dir.file_name().is_some_and(|n| n == "build") {
            // parent is target/{profile}
            return Some(parent.to_path_buf());
        }
        dir = parent;
    }
    None
}
