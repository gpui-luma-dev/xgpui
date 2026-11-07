//! End-to-end release CLI tests with local Git repositories and no network.
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
fn tool(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xgpui-release"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}
// The CLI command remains the first argument; --root is a common option after it.
fn run(root: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xgpui-release"));
    command.args(args).arg("--root").arg(root).output().unwrap()
}
fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for dir in ["scripts", ".cargo", "gpui/crates/gpui/src"] {
        fs::create_dir_all(root.join(dir)).unwrap();
    }
    for file in ["Cargo.toml", ".cargo/config.toml", ".cargo/validation.toml"] {
        fs::write(root.join(file), "").unwrap();
    }
    fs::write(
        root.join("scripts/release-packages.json"),
        json!({"packages":{"gpui":"xgpui"},"version":"1.0.0","versions":{}}).to_string(),
    )
    .unwrap();
    fs::write(
        root.join("upstream.json"),
        json!({"packages":{"gpui":"gpui/crates/gpui"}}).to_string(),
    )
    .unwrap();
    fs::write(root.join("gpui/crates/gpui/Cargo.toml"),"[package]\nname='xgpui'\nversion='1.0.0'\ndescription='fixture'\nlicense='MIT'\nrepository='https://example.test'\n").unwrap();
    fs::write(
        root.join("gpui/crates/gpui/src/lib.rs"),
        "pub fn sample() {}\n",
    )
    .unwrap();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "fixture@example.test"]);
    git(root, &["config", "user.name", "Fixture"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fixture"]);
    git(root, &["tag", "release-epoch"]);
    temp
}
#[test]
fn manifest_inventory_and_prepare_plan_work_without_python() {
    let fixture = fixture();
    let root = fixture.path();
    let result = run(root, &["preflight"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let doc: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(doc["packages"][0]["name"], "xgpui");
    assert_eq!(doc["release_ready"], false);
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("plan");
    let result = run(root, &["prepare-registry", "--out", path.to_str().unwrap()]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let doc: Value =
        serde_json::from_slice(&fs::read(path.join("release-plan.json")).unwrap()).unwrap();
    assert_eq!(doc["preparation_status"], "passed");
    assert_eq!(doc["uploads_enabled"], false);
    assert!(!path.join("target").exists());
}
#[test]
fn daily_prepare_writes_reports_without_changing_versions() {
    let fixture = fixture();
    let root = fixture.path();
    fs::write(
        root.join("gpui/crates/gpui/src/lib.rs"),
        "pub fn changed() {}\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "change"]);
    let out = tempfile::tempdir().unwrap();
    let report = out.path().join("preparation.json");
    let notes = out.path().join("notes.md");
    let result = run(
        root,
        &[
            "daily",
            "prepare",
            "--baseline",
            "release-epoch",
            "--report",
            report.to_str().unwrap(),
            "--notes",
            notes.to_str().unwrap(),
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let doc: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(doc["packages"][0]["new"], "1.0.1");
    assert!(
        fs::read_to_string(notes)
            .unwrap()
            .contains("Review required")
    );
    assert!(
        fs::read_to_string(root.join("gpui/crates/gpui/Cargo.toml"))
            .unwrap()
            .contains("version='1.0.0'")
    );
}
#[test]
fn publish_requires_confirmation_before_staging_or_network() {
    let fixture = fixture();
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("release");
    let result = Command::new(env!("CARGO_BIN_EXE_xgpui-release"))
        .args([
            "registry-release",
            "--mode",
            "publish",
            "--confirmation",
            "publish wrong",
            "--out",
        ])
        .arg(&path)
        .arg("--root")
        .arg(fixture.path())
        .env("CARGO_REGISTRY_TOKEN", "test-secret-token")
        .output()
        .unwrap();
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("exact confirmation"));
    assert!(!stderr.contains("test-secret-token"));
    assert!(!path.exists());
}
#[test]
fn output_inside_checkout_rejected_without_creating_it() {
    let fixture = fixture();
    let path = fixture.path().join("output");
    let result = run(
        fixture.path(),
        &["prepare-registry", "--out", path.to_str().unwrap()],
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("outside"));
    assert!(!path.exists());
}
#[test]
fn help_is_non_mutating() {
    let fixture = fixture();
    let result = tool(fixture.path(), &["--help"]);
    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("registry-release"));
}
