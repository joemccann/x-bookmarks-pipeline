use std::env;
use std::fs;
use std::process::Command;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn bootstrap_uses_temp_dotenv_and_performs_dry_run() {
    let temp_root = env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_nanos())
        .unwrap_or(0);
    let workspace = temp_root.join(format!("xbp-dry-run-{nanos}"));
    fs::create_dir_all(&workspace).unwrap();

    let env_path = workspace.join(".env");
    fs::write(
        &env_path,
        concat!(
            "CEREBRAS_API_KEY=cb_test_key\n",
            "XAI_API_KEY=xai_test_key\n",
            "ANTHROPIC_API_KEY=anthropic_test_key\n",
            "OPENAI_API_KEY=openai_test_key\n",
        ),
    )
    .unwrap();

    let cache_path = workspace.join("bootstrap-cache.db");
    let binary = env::var("CARGO_BIN_EXE_x_bookmarks_pipeline_rust")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            path.push("target");
            path.push("debug");
            #[cfg(windows)]
            path.push("x-bookmarks-pipeline-rust.exe");
            #[cfg(not(windows))]
            path.push("x-bookmarks-pipeline-rust");
            path
        });
    assert!(binary.exists(), "expected built binary at {binary:?}");

    let output = Command::new(binary)
        .current_dir(&workspace)
        .arg("--cache-path")
        .arg(cache_path)
        .arg("--clear-cache")
        .env_remove("CEREBRAS_API_KEY")
        .env_remove("XAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("cache cleared"));
    assert!(
        workspace.join("cache").exists() == false,
        "bootstrap should not require default cache path fallback"
    );
}
