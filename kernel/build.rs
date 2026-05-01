use std::process::Command;

fn main() {
    // Build timestamp
    let timestamp = if cfg!(target_os = "windows") {
        // Windows: use PowerShell
        Command::new("powershell")
            .args(["-Command", "Get-Date -Format 'yyyy-MM-dd HH:mm:ss'"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    } else {
        // Unix: use date
        Command::new("date")
            .args(["+%Y-%m-%d %H:%M:%S"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    };

    println!("cargo:rustc-env=BUILD_TIMESTAMP={}", timestamp);

    // Git hash (optional)
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=GIT_HASH={}", git_hash);

    // Re-run if git HEAD changes
    println!("cargo:rerun-if-changed=.git/HEAD");
}
