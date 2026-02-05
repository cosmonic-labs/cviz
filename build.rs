use std::process::Command;

fn main() {
    // Get git short SHA
    let git_sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=CVIZ_GIT_SHA={}", git_sha);

    // Extract wasmparser version from Cargo.lock
    let lock_contents = std::fs::read_to_string("Cargo.lock").unwrap_or_default();
    let wasmparser_version = parse_wasmparser_version(&lock_contents)
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=WASMPARSER_VERSION={}", wasmparser_version);

    // Re-run if git HEAD or Cargo.lock changes
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=Cargo.lock");
}

fn parse_wasmparser_version(lock_contents: &str) -> Option<String> {
    let mut lines = lock_contents.lines();
    while let Some(line) = lines.next() {
        if line.starts_with("name = \"wasmparser\"") {
            if let Some(version_line) = lines.next() {
                return version_line
                    .strip_prefix("version = \"")
                    .and_then(|s| s.strip_suffix('"'))
                    .map(|s| s.to_string());
            }
        }
    }
    None
}
