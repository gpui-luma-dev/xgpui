//! Offline repository fixture shared by release-policy regression tests.
use super::*;
pub(super) fn repository() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    git(root, &["init", "-q"]).unwrap();
    git(root, &["config", "user.email", "fixture@example.test"]).unwrap();
    git(root, &["config", "user.name", "Fixture"]).unwrap();
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::create_dir_all(root.join(".cargo")).unwrap();
    for file in ["Cargo.toml", ".cargo/config.toml", ".cargo/validation.toml"] {
        fs::write(root.join(file), "").unwrap();
    }
    write_json(root.join("scripts/release-packages.json"),&json!({"packages":{"gpui":"xgpui","gpui-web":"xgpui-web","xgpui-color":"xgpui-color"},"version":"1.0.0","versions":{}})).unwrap();
    write_json(
        root.join("upstream.json"),
        &json!({"packages":{"gpui":"gpui/crates/gpui","gpui-web":"gpui/crates/gpui-web"}}),
    )
    .unwrap();
    write_json(
        root.join("scripts/release-check.json"),
        &json!({"epoch":"release-epoch","repository":"owner/repo"}),
    )
    .unwrap();
    for (name, path) in [
        ("xgpui", "gpui/crates/gpui"),
        ("xgpui-web", "gpui/crates/gpui-web"),
        ("xgpui-color", "extensions/crates/xgpui-color"),
    ] {
        fs::create_dir_all(root.join(path).join("src")).unwrap();
        fs::write(root.join(path).join("src/lib.rs"), "pub fn sample() {}\n").unwrap();
        fs::write(root.join(path).join("Cargo.toml"),format!("[package]\nname='{name}'\nversion='1.0.0'\ndescription='fixture'\nlicense='MIT'\nrepository='https://example.test'\n{}",if name=="xgpui-web"{"[dependencies]\ngpui={package='xgpui',version='1.0.0',path='../gpui'}\n"}else{""})).unwrap();
    }
    commit(root);
    git(root, &["tag", "release-epoch"]).unwrap();
    temp
}
pub(super) fn commit(root: &Path) {
    git(root, &["add", "."]).unwrap();
    git(root, &["commit", "-qm", "fixture"]).unwrap();
}
pub(super) fn bump(root: &Path, name: &str, version: &str) {
    let manifests = inventory::manifests(root).unwrap();
    let path = root.join(&manifests[name]);
    let text = fs::read_to_string(&path).unwrap().replacen(
        "version='1.0.0'",
        &format!("version='{version}'"),
        1,
    );
    fs::write(path, text).unwrap();
    let path = root.join("scripts/release-packages.json");
    let mut map = read_json(&path).unwrap();
    map["versions"][name] = json!(version);
    write_json(path, &map).unwrap();
}
