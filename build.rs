// build.rs
use std::{fs, process::Command};

fn main() {
    // Always use COMMIT_HASH file as primary source
    let commit_hash = fs::read_to_string("COMMIT_HASH")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| {
            eprintln!("Error: COMMIT_HASH file missing.");
            std::process::exit(1);
        });

    // If .git is available, validate COMMIT_HASH
    let git_dir_exists = fs::metadata(".git").is_ok();
    if git_dir_exists {
        let git_hash = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()
            .and_then(|out| if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            } else { None });
        if let Some(git_hash) = git_hash {
            if git_hash != commit_hash {
                eprintln!("Error: COMMIT_HASH ({}) does not match current git hash ({}).", commit_hash, git_hash);
                std::process::exit(1);
            }
        }
    }

    println!("cargo:rustc-env=GIT_HASH={}", commit_hash);
}
