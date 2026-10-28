//! Repeatable downstream naming and native harness generation, without publishing.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};
use toml_edit::{DocumentMut, Item, Value};
use walkdir::WalkDir;

type Names = BTreeMap<String, String>;

fn read_names(path: &Path) -> Result<Names> {
    let names: Names = serde_json::from_str(&fs::read_to_string(path)?)?;
    let mut targets = BTreeSet::new();
    for (old, new) in &names {
        ensure!(
            old == "gpui-unofficial" || old.ends_with("-gpui-unofficial"),
            "Unexpected source name {old}"
        );
        ensure!(
            !new.is_empty()
                && new
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'),
            "Invalid target name {new}"
        );
        ensure!(targets.insert(new), "Duplicate normalized name {new}");
    }
    Ok(names)
}

fn rewrite(text: &str, names: &Names) -> String {
    // Attribution and registry/Git URLs continue to identify the real upstream.
    if text.contains("://") {
        return text.to_owned();
    }
    let mut result = String::new();
    let mut token = String::new();
    for c in text.chars().chain(std::iter::once('\0')) {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            token.push(c);
        } else {
            result.push_str(names.get(&token).unwrap_or(&token));
            token.clear();
            if c != '\0' {
                result.push(c);
            }
        }
    }
    result
}

fn rewrite_value(value: &mut Value, names: &Names) -> Result<()> {
    match value {
        Value::String(s) => {
            let old = s.value();
            let new = rewrite(old, names);
            if &new != old {
                let decor = s.decor().clone();
                *value = Value::from(new);
                *value.decor_mut() = decor;
            }
        }
        Value::Array(a) => {
            for v in a.iter_mut() {
                rewrite_value(v, names)?;
            }
        }
        Value::InlineTable(t) => {
            let keys: Vec<_> = t.iter().map(|(k, _)| k.to_owned()).collect();
            for old in keys {
                let new = rewrite(&old, names);
                if new != old {
                    ensure!(
                        !t.contains_key(&new),
                        "Dependency key collision: {old} -> {new}"
                    );
                    let item = t.remove(&old).context("Missing inline item")?;
                    t.insert(&new, item);
                }
                rewrite_value(t.get_mut(&new).context("Missing inline value")?, names)?;
            }
        }
        _ => (),
    }
    Ok(())
}
fn rewrite_item(item: &mut Item, names: &Names) -> Result<()> {
    match item {
        Item::Value(v) => rewrite_value(v, names)?,
        Item::Table(t) => {
            let keys: Vec<_> = t.iter().map(|(k, _)| k.to_owned()).collect();
            for old in keys {
                let new = rewrite(&old, names);
                if new != old {
                    ensure!(!t.contains_key(&new), "Table key collision: {old} -> {new}");
                    let item = t.remove(&old).context("Missing table item")?;
                    t.insert(&new, item);
                }
                rewrite_item(t.get_mut(&new).context("Missing table value")?, names)?;
            }
        }
        Item::ArrayOfTables(a) => {
            for table in a.iter_mut() {
                let mut item = Item::Table(table.clone());
                rewrite_item(&mut item, names)?;
                *table = item.as_table().context("Expected rewritten table")?.clone();
            }
        }
        Item::None => (),
    }
    Ok(())
}

fn safe_relative(path: &str) -> Result<&Path> {
    let path = Path::new(path);
    ensure!(
        path.components().all(|c| matches!(c, Component::Normal(_))),
        "Unsafe captured path {}",
        path.display()
    );
    ensure!(
        path.starts_with("gpui/crates"),
        "Package outside capture: {}",
        path.display()
    );
    Ok(path)
}

// Supporting crates keep their upstream package identity so all consumers share types.
fn registry_crates(root: &Path) -> Result<Names> {
    let path = root.join("scripts/registry-crates.json");
    if path.exists() {
        Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
    } else {
        Ok(Names::new())
    }
}

fn release_packages(root: &Path) -> Result<(String, Names, Names)> {
    let path = root.join("scripts/release-packages.json");
    if !path.exists() {
        return Ok((String::new(), Names::new(), Names::new()));
    }
    let config: Json = serde_json::from_str(&fs::read_to_string(path)?)?;
    let version = config["version"]
        .as_str()
        .context("Missing downstream version")?
        .to_owned();
    ensure!(
        version.split('.').count() == 3
            && version
                .split('.')
                .all(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())),
        "Invalid downstream version"
    );
    let packages: Names = serde_json::from_value(config["packages"].clone())?;
    let mut unique = BTreeSet::new();
    for name in packages.values() {
        ensure!(
            (name == "xgpui" || name.starts_with("xgpui-"))
                && name
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                && unique.insert(name),
            "Invalid or duplicate release package {name}"
        );
    }
    let versions: Names =
        serde_json::from_value(config.get("versions").cloned().unwrap_or_else(|| json!({})))?;
    for (name, override_version) in &versions {
        ensure!(
            packages.values().any(|p| p == name),
            "Version override for unknown package {name}"
        );
        ensure!(
            override_version.split('.').count() == 3
                && override_version
                    .split('.')
                    .all(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())),
            "Invalid version override for {name}"
        );
    }
    Ok((version, packages, versions))
}

fn prepare_manifest(
    document: &mut DocumentMut,
    registry: &Names,
    upstream_version: &str,
    release: &Names,
    release_version: &str,
    versions: &Names,
) -> Result<()> {
    externalize(document.as_item_mut(), registry, upstream_version, true)?;
    for (local, published) in release {
        let version = versions
            .get(published)
            .map(String::as_str)
            .unwrap_or(release_version);
        externalize(
            document.as_item_mut(),
            &BTreeMap::from([(local.clone(), published.clone())]),
            version,
            false,
        )?;
    }
    if let Some(name) = document
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(Item::as_str)
        .map(str::to_owned)
    {
        if let Some((local, published)) = release
            .iter()
            .find(|(local, published)| **local == name || **published == name)
        {
            document["package"]["name"] = toml_edit::value(published.as_str());
            document["package"]["version"] = toml_edit::value(
                versions
                    .get(published)
                    .map(String::as_str)
                    .unwrap_or(release_version),
            );
            document["package"]["repository"] =
                toml_edit::value("https://github.com/gpui-luma-dev/xgpui");
            // Library names and dependency aliases preserve existing source imports.
            if local.starts_with("gpui") {
                document["lib"]["name"] = toml_edit::value(local.replace('-', "_"));
            }
            // The platform depends on core. Keep core's test/example-only edge
            // local so Cargo omits it from the registry manifest on first release.
            if local == "gpui" {
                if let Some(options) = document
                    .get_mut("dev-dependencies")
                    .and_then(|deps| deps.get_mut("gpui_platform"))
                    .and_then(Item::as_table_like_mut)
                {
                    options.insert("path", toml_edit::value("../gpui-platform"));
                    options.remove("version");
                }
            }
        }
    }
    Ok(())
}

fn externalize(item: &mut Item, registry: &Names, version: &str, remove_paths: bool) -> Result<()> {
    if let Some(table) = item.as_table_like_mut() {
        for (key, value) in table.iter_mut() {
            if matches!(
                key.get(),
                "dependencies" | "dev-dependencies" | "build-dependencies"
            ) {
                if let Some(dependencies) = value.as_table_like_mut() {
                    for (alias, dependency) in dependencies.iter_mut() {
                        let package = dependency
                            .get("package")
                            .and_then(Item::as_str)
                            .unwrap_or(alias.get())
                            .to_owned();
                        if let Some(published) = registry
                            .get(&package)
                            .or_else(|| registry.values().find(|name| **name == package))
                        {
                            if dependency.is_str() {
                                let mut options = toml_edit::InlineTable::new();
                                options.insert("version", Value::from(format!("={version}")));
                                *dependency = Item::Value(Value::InlineTable(options));
                            }
                            let options = dependency
                                .as_table_like_mut()
                                .context("Invalid registry dependency")?;
                            if remove_paths {
                                for field in ["path", "git", "rev", "branch", "tag", "registry"] {
                                    options.remove(field);
                                }
                            }
                            options.insert("package", toml_edit::value(published.as_str()));
                            options.insert("version", toml_edit::value(format!("={version}")));
                        }
                    }
                }
            } else {
                externalize(value, registry, version, remove_paths)?;
            }
        }
    }
    Ok(())
}

fn normalize(root: &Path, names: &Names, check: bool) -> Result<Json> {
    let mut provenance: Json =
        serde_json::from_str(&fs::read_to_string(root.join("upstream.json"))?)?;
    let source = provenance
        .get("source_packages")
        .unwrap_or(&provenance["packages"])
        .as_object()
        .context("Missing package map")?
        .clone();
    let registry = registry_crates(root)?;
    let (release_version, release, versions) = release_packages(root)?;
    let core_path = source
        .get("gpui-unofficial")
        .and_then(Json::as_str)
        .context("Missing upstream core")?;
    let raw_core = root.join(core_path).join("Cargo.toml");
    let core_manifest = if raw_core.exists() {
        raw_core
    } else {
        root.join("gpui/crates/gpui/Cargo.toml")
    };
    let core: toml::Value = toml::from_str(&fs::read_to_string(core_manifest)?)?;
    let upstream_version = provenance
        .get("ref")
        .and_then(Json::as_str)
        .and_then(|r| r.strip_prefix("release/"))
        .or_else(|| core["package"]["version"].as_str())
        .context("Missing upstream release version")?
        .to_owned();
    let version = upstream_version.as_str();
    let mut packages = BTreeMap::new();
    let mut removals = Vec::new();
    for (old, value) in &source {
        let new = names
            .get(old)
            .with_context(|| format!("Unmapped crate {old}; review scripts/crate-names.json"))?;
        if registry.contains_key(new) {
            ensure!(
                registry[new] == *old,
                "Registry identity mismatch for {new}"
            );
            for path in [
                value.as_str().context("Invalid source path")?.to_owned(),
                format!("gpui/crates/{new}"),
            ] {
                safe_relative(&path)?;
                let path = root.join(path);
                if path.exists() {
                    removals.push(path);
                }
            }
            continue;
        }
        let original = value.as_str().context("Invalid source path")?;
        safe_relative(original)?;
        let path = rewrite(original, names);
        safe_relative(&path)?;
        ensure!(
            packages.insert(new.clone(), path).is_none(),
            "Duplicate target {new}"
        );
    }
    ensure!(packages.contains_key("gpui"), "Missing core gpui package");
    let mut writes = Vec::new();
    let mut moves = Vec::new();
    let staging = root.join("gpui/crates/transformed-crates");
    for entry in WalkDir::new(root.join("gpui"))
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.path() != staging && !removals.iter().any(|p| entry.path().starts_with(p))
        })
    {
        let entry = entry?;
        let path = entry.path();
        ensure!(
            !entry.file_type().is_symlink(),
            "Review symlink before normalization: {}",
            path.display()
        );
        if entry.file_type().is_dir() {
            if let Some(new) = entry.file_name().to_str().and_then(|n| names.get(n)) {
                let target = path.with_file_name(new);
                ensure!(
                    !target.exists(),
                    "Directory collision: {}",
                    target.display()
                );
                moves.push((path.to_owned(), target));
            }
        } else if path.extension().is_some_and(|e| e == "toml") {
            let old = fs::read_to_string(path)?;
            let mut document = old
                .parse::<DocumentMut>()
                .with_context(|| format!("Parse {}", path.display()))?;
            if let Some(name) = document
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(Item::as_str)
            {
                ensure!(
                    !(name == "gpui-unofficial" || name.ends_with("-gpui-unofficial"))
                        || names.contains_key(name),
                    "Unmapped manifest package {name}"
                );
            }
            rewrite_item(document.as_item_mut(), names)?;
            prepare_manifest(
                &mut document,
                &registry,
                version,
                &release,
                &release_version,
                &versions,
            )?;
            let new = document.to_string();
            if old != new {
                writes.push((path.to_owned(), new));
            }
        }
    }
    // Extension/app aliases must resolve to the same registry types as the framework.
    for directory in ["apps", "extensions"] {
        if !root.join(directory).exists() {
            continue;
        }
        for entry in WalkDir::new(root.join(directory))
            .into_iter()
            .filter_entry(|entry| entry.file_name() != "target" && entry.file_name() != ".git")
        {
            let entry = entry?;
            if entry.file_name() != "Cargo.toml" {
                continue;
            }
            let old = fs::read_to_string(entry.path())?;
            let mut document = old.parse::<DocumentMut>()?;
            prepare_manifest(
                &mut document,
                &registry,
                version,
                &release,
                &release_version,
                &versions,
            )?;
            let new = document.to_string();
            if old != new {
                writes.push((entry.path().to_owned(), new));
            }
        }
    }
    // Validate every expected manifest and identity before touching the capture.
    for (old, value) in &source {
        if registry.contains_key(&names[old]) {
            continue;
        }
        let raw = root.join(value.as_str().context("Invalid source path")?);
        let normalized = root.join(&packages[&names[old]]);
        let present = if raw.is_dir() { raw } else { normalized };
        let doc: toml::Value = toml::from_str(&fs::read_to_string(present.join("Cargo.toml"))?)?;
        let actual = doc["package"]["name"]
            .as_str()
            .context("Missing package name")?;
        ensure!(
            actual == old
                || actual == names[old]
                || release.get(&names[old]).is_some_and(|name| name == actual),
            "Package identity mismatch at {}",
            present.display()
        );
    }
    if !release.is_empty() {
        provenance["distribution"] =
            json!({"version": release_version, "packages": release, "versions": versions});
    }
    provenance["source_packages"] = Json::Object(source);
    provenance["packages"] = serde_json::to_value(&packages)?;
    if !registry.is_empty() {
        provenance["registry_packages"] = json!({"version": version, "packages": registry});
    }
    provenance["normalization"] = json!({"schema_version":1,"crate_names":names});
    let metadata = serde_json::to_string_pretty(&provenance)? + "\n";
    let mut config =
        String::from("# Generated by xgpui-release normalize; do not edit.\n[patch.crates-io]\n");
    for (name, path) in &packages {
        config += &format!(
            "{} = {{ path = {} }}\n",
            serde_json::to_string(release.get(name).unwrap_or(name))?,
            serde_json::to_string(path)?
        );
    }
    if root.join("vendor/block/Cargo.toml").is_file() {
        config += "block = { path = \"vendor/block\" }\n";
    }
    for (path, text) in [
        (root.join("upstream.json"), metadata),
        (root.join(".cargo/config.toml"), config),
    ] {
        if fs::read_to_string(&path).unwrap_or_default() != text {
            writes.push((path, text));
        }
    }
    let report = json!({"packages":packages.len(),"renamed_directories":moves.len(),"rewritten_files":writes.len()});
    if check {
        ensure!(
            writes.is_empty() && moves.is_empty() && removals.is_empty() && !staging.exists(),
            "Normalization is stale: {report}"
        );
        return Ok(report);
    }
    if staging.exists() {
        ensure!(
            !staging.is_symlink(),
            "Staging mirror must not be a symlink"
        );
        fs::remove_dir_all(&staging)?;
    }
    for path in removals {
        fs::remove_dir_all(path)?;
    }
    for (path, text) in writes {
        fs::write(path, text)?;
    }
    moves.sort_by_key(|(old, _)| std::cmp::Reverse(old.components().count()));
    for (old, new) in moves {
        fs::rename(old, new)?;
    }
    Ok(report)
}

fn canonical(name: &str) -> &str {
    if name == "gpui-unofficial" {
        "gpui"
    } else {
        name.strip_suffix("-gpui-unofficial").unwrap_or(name)
    }
}
fn build_manifests(root: &Path, check: bool) -> Result<()> {
    let (_, release, _) = release_packages(root)?;
    let provenance: Json = serde_json::from_str(&fs::read_to_string(root.join("upstream.json"))?)?;
    let packages: BTreeMap<String, String> =
        serde_json::from_value(provenance["packages"].clone())?;
    let validation: toml::Value =
        toml::from_str(&fs::read_to_string(root.join(".cargo/validation.toml"))?)?;
    let header = "# Generated by xgpui-release build-manifests; do not edit.\n";
    let mut workspace = format!(
        "{header}[workspace]\nresolver = \"2\"\nmembers = [\"tools/build-all\"]\nexclude = [\n"
    );
    for (name, path) in &packages {
        safe_relative(path)?;
        let manifest: toml::Value =
            toml::from_str(&fs::read_to_string(root.join(path).join("Cargo.toml"))?)?;
        ensure!(
            manifest["package"]["name"].as_str() == Some(release.get(name).unwrap_or(name)),
            "Stale package map for {name}"
        );
        workspace += &format!("    {},\n", serde_json::to_string(path)?);
    }
    workspace += "]\n\n[patch.crates-io]\n";
    for (name, options) in validation["patch"]["crates-io"]
        .as_table()
        .context("Missing compatibility patches")?
    {
        // Proptest's fork is required by standalone test validation only.
        // Ordinary library builds do not activate it and Cargo warns on the unused patch.
        if name == "proptest" {
            continue;
        }
        let fields = options
            .as_table()
            .context("Invalid compatibility patch")?
            .iter()
            .map(|(k, v)| format!("{k} = {v}"))
            .collect::<Vec<_>>()
            .join(", ");
        workspace += &format!("{name} = {{ {fields} }}\n");
    }
    let find = |wanted: &str| -> Result<&str> {
        packages
            .keys()
            .find(|n| canonical(n) == wanted)
            .map(String::as_str)
            .with_context(|| format!("Missing {wanted}"))
    };
    let platform = find("gpui-platform")?;
    let mut harness = format!(
        "{header}[package]\nname = \"xgpui-build\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[features]\nruntime-shaders = [\n"
    );
    for wanted in ["gpui-platform", "gpui-apple", "gpui-macos"] {
        let weak = if wanted == "gpui-platform" { "" } else { "?" };
        harness += &format!("    \"{}{weak}/runtime_shaders\",\n", find(wanted)?);
    }
    harness += "]\nfull-capture = [\n";
    for name in packages
        .keys()
        .filter(|n| !matches!(canonical(n), "gpui" | "gpui-platform"))
    {
        harness += &format!("    \"dep:{name}\",\n");
    }
    harness += "]\n\n[dependencies]\n";
    let targets = BTreeMap::from([
        (
            "gpui-apple",
            "any(target_os = \"macos\", target_os = \"ios\")",
        ),
        ("gpui-macos", "target_os = \"macos\""),
        (
            "gpui-linux",
            "any(target_os = \"linux\", target_os = \"freebsd\")",
        ),
        ("gpui-windows", "target_os = \"windows\""),
        ("gpui-web", "target_family = \"wasm\""),
    ]);
    let dependency = |name: &str| {
        let optional = if matches!(canonical(name), "gpui" | "gpui-platform") {
            ""
        } else {
            ", optional = true"
        };
        let package = release
            .get(name)
            .map(|p| format!(", package = {p:?}"))
            .unwrap_or_default();
        format!(
            "{name} = {{ path = \"../../{}\"{package}{optional} }}\n",
            packages[name]
        )
    };
    for name in packages
        .keys()
        .filter(|n| !targets.contains_key(canonical(n)))
    {
        harness += &dependency(name);
    }
    for (name, path) in &packages {
        if let Some(condition) = targets.get(canonical(name)) {
            harness += &format!("\n[target.'cfg({condition})'.dependencies]\n");
            harness += &dependency(name);
            let features = match canonical(name) {
                "gpui-macos" => Some("[\"font-kit\"]"),
                "gpui-linux" => Some("[\"wayland\", \"x11\"]"),
                _ => None,
            };
            if let Some(features) = features {
                let package = release
                    .get(platform)
                    .map(|p| format!(", package = {p:?}"))
                    .unwrap_or_default();
                harness += &format!(
                    "{platform} = {{ path = \"../../{}\", features = {features}{package} }}\n",
                    packages[platform]
                );
            }
        }
        let _ = path;
    }
    for (file, text) in [
        ("Cargo.toml", workspace),
        ("tools/build-all/Cargo.toml", harness),
    ] {
        if check {
            ensure!(
                fs::read_to_string(root.join(file))? == text,
                "Stale {file}; run xgpui-release build-manifests"
            );
        } else {
            fs::write(root.join(file), text)?;
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from)
    };
    let root = value("--root")
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .canonicalize()?;
    let check = args.iter().any(|a| a == "--check");
    match args.first().map(String::as_str) {
        Some("normalize") => {
            let names = read_names(
                &value("--map").unwrap_or_else(|| root.join("scripts/crate-names.json")),
            )?;
            println!("{}", normalize(&root, &names, check)?);
        }
        Some("build-manifests") => build_manifests(&root, check)?,
        _ => bail!(
            "Usage: xgpui-release normalize|build-manifests [--root PATH] [--map PATH] [--check]"
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Names) {
        let dir = tempfile::tempdir().expect("temporary fixture");
        let root = dir.path();
        fs::create_dir_all(root.join("gpui/crates/gpui-unofficial/src")).unwrap();
        fs::create_dir_all(root.join(".cargo")).unwrap();
        fs::write(root.join("gpui/crates/gpui-unofficial/Cargo.toml"),"[package]\nname = 'gpui-unofficial'\nversion = '1.0.0'\nrepository = 'https://github.com/gpui-archipelago/gpui-unofficial'\n[dependencies]\nhelper = { package = 'helper-gpui-unofficial', path = '../helper-gpui-unofficial', optional = true }\n[features]\nhelper-support = ['helper/feature']\n[target.'cfg(unix)'.build-dependencies]\nhelper = { package = 'helper-gpui-unofficial', version = '1' }\n").unwrap();
        fs::write(
            root.join("gpui/crates/gpui-unofficial/src/lib.rs"),
            "// source must stay unchanged\npub fn value() -> u8 { 7 }\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("gpui/crates/helper-gpui-unofficial")).unwrap();
        fs::write(
            root.join("gpui/crates/helper-gpui-unofficial/Cargo.toml"),
            "[package]\nname='helper-gpui-unofficial'\nversion='1.0.0'\n",
        )
        .unwrap();
        fs::write(root.join("upstream.json"),json!({"repository":"https://github.com/gpui-archipelago/gpui-unofficial","commit":"fixture-sha","packages":{"gpui-unofficial":"gpui/crates/gpui-unofficial","helper-gpui-unofficial":"gpui/crates/helper-gpui-unofficial"}}).to_string()).unwrap();
        (
            dir,
            BTreeMap::from([
                ("gpui-unofficial".into(), "gpui".into()),
                ("helper-gpui-unofficial".into(), "helper".into()),
            ]),
        )
    }
    #[test]
    fn normalizes_targeted_dependencies_preserves_aliases_source_and_provenance() {
        let (dir, names) = fixture();
        let root = dir.path();
        normalize(root, &names, false).unwrap();
        let text = fs::read_to_string(root.join("gpui/crates/gpui/Cargo.toml")).unwrap();
        let manifest: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(
            manifest["dependencies"]["helper"]["path"].as_str(),
            Some("../helper")
        );
        assert_eq!(
            manifest["target"]["cfg(unix)"]["build-dependencies"]["helper"]["package"].as_str(),
            Some("helper")
        );
        assert_eq!(
            manifest["features"]["helper-support"][0].as_str(),
            Some("helper/feature")
        );
        assert!(text.contains("https://github.com/gpui-archipelago/gpui-unofficial"));
        assert!(
            fs::read_to_string(root.join("gpui/crates/gpui/src/lib.rs"))
                .unwrap()
                .contains("pub fn value")
        );
        let data: Json =
            serde_json::from_str(&fs::read_to_string(root.join("upstream.json")).unwrap()).unwrap();
        assert_eq!(data["commit"], "fixture-sha");
        assert!(data["source_packages"].get("gpui-unofficial").is_some());
        normalize(root, &names, true).unwrap();
        normalize(root, &names, false).unwrap();
        normalize(root, &names, true).unwrap();
    }
    #[test]
    fn registry_helpers_preserve_aliases_features_and_are_removed_idempotently() {
        let (dir, names) = fixture();
        let root = dir.path();
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(
            root.join("scripts/registry-crates.json"),
            r#"{"helper":"helper-gpui-unofficial"}"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("extensions/example")).unwrap();
        fs::write(
            root.join("extensions/example/Cargo.toml"),
            "[dependencies]\nhelper={path='../../gpui/crates/helper',features=['test-support']}\n",
        )
        .unwrap();
        normalize(root, &names, false).unwrap();
        assert!(!root.join("gpui/crates/helper-gpui-unofficial").exists());
        let manifest: toml::Value =
            toml::from_str(&fs::read_to_string(root.join("gpui/crates/gpui/Cargo.toml")).unwrap())
                .unwrap();
        let helper = &manifest["dependencies"]["helper"];
        assert_eq!(helper["package"].as_str(), Some("helper-gpui-unofficial"));
        assert_eq!(helper["version"].as_str(), Some("=1.0.0"));
        assert_eq!(helper["optional"].as_bool(), Some(true));
        assert!(helper.get("path").is_none());
        assert_eq!(
            manifest["target"]["cfg(unix)"]["build-dependencies"]["helper"]["package"].as_str(),
            Some("helper-gpui-unofficial")
        );
        assert_eq!(
            manifest["features"]["helper-support"][0].as_str(),
            Some("helper/feature")
        );
        let extension: toml::Value = toml::from_str(
            &fs::read_to_string(root.join("extensions/example/Cargo.toml")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            extension["dependencies"]["helper"]["features"][0].as_str(),
            Some("test-support")
        );
        let provenance: Json =
            serde_json::from_str(&fs::read_to_string(root.join("upstream.json")).unwrap()).unwrap();
        assert!(provenance["packages"].get("helper").is_none());
        assert_eq!(provenance["registry_packages"]["version"], "1.0.0");
        // Already-externalized app dependencies must also follow a release update.
        let extension_path = root.join("extensions/example/Cargo.toml");
        let stale = fs::read_to_string(&extension_path)
            .unwrap()
            .replace("=1.0.0", "=0.9.0");
        fs::write(&extension_path, stale).unwrap();
        assert!(normalize(root, &names, true).is_err());
        normalize(root, &names, false).unwrap();
        assert!(
            fs::read_to_string(extension_path)
                .unwrap()
                .contains("=1.0.0")
        );

        assert!(
            provenance["source_packages"]
                .get("helper-gpui-unofficial")
                .is_some()
        );
        normalize(root, &names, true).unwrap();
        normalize(root, &names, false).unwrap();
        normalize(root, &names, true).unwrap();
    }

    #[test]
    fn downstream_version_and_names_are_independent_of_upstream_helpers() {
        let (dir, names) = fixture();
        let root = dir.path();
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(
            root.join("scripts/registry-crates.json"),
            r#"{"helper":"helper-gpui-unofficial"}"#,
        )
        .unwrap();
        fs::write(
            root.join("scripts/release-packages.json"),
            r#"{"version":"1.0.0","packages":{"gpui":"xgpui"}}"#,
        )
        .unwrap();
        let mut data: Json =
            serde_json::from_str(&fs::read_to_string(root.join("upstream.json")).unwrap()).unwrap();
        data["ref"] = json!("release/1.25.3");
        fs::write(root.join("upstream.json"), data.to_string()).unwrap();
        fs::create_dir_all(root.join("apps/example")).unwrap();
        fs::write(
            root.join("apps/example/Cargo.toml"),
            "[dependencies]\ngpui={path='../../gpui/crates/gpui',features=['test-support']}\n",
        )
        .unwrap();
        normalize(root, &names, false).unwrap();
        let manifest: toml::Value =
            toml::from_str(&fs::read_to_string(root.join("gpui/crates/gpui/Cargo.toml")).unwrap())
                .unwrap();
        assert_eq!(manifest["package"]["name"].as_str(), Some("xgpui"));
        assert_eq!(manifest["package"]["version"].as_str(), Some("1.0.0"));
        assert_eq!(manifest["lib"]["name"].as_str(), Some("gpui"));
        assert_eq!(
            manifest["dependencies"]["helper"]["version"].as_str(),
            Some("=1.25.3")
        );
        let app: toml::Value =
            toml::from_str(&fs::read_to_string(root.join("apps/example/Cargo.toml")).unwrap())
                .unwrap();
        assert_eq!(
            app["dependencies"]["gpui"]["package"].as_str(),
            Some("xgpui")
        );
        assert_eq!(
            app["dependencies"]["gpui"]["path"].as_str(),
            Some("../../gpui/crates/gpui")
        );
        assert_eq!(
            app["dependencies"]["gpui"]["features"][0].as_str(),
            Some("test-support")
        );
        let config = fs::read_to_string(root.join(".cargo/config.toml")).unwrap();
        assert!(config.contains("xgpui"));
        let data: Json =
            serde_json::from_str(&fs::read_to_string(root.join("upstream.json")).unwrap()).unwrap();
        assert_eq!(data["distribution"]["version"], "1.0.0");
        assert_eq!(data["registry_packages"]["version"], "1.25.3");
        normalize(root, &names, true).unwrap();
        normalize(root, &names, false).unwrap();
        normalize(root, &names, true).unwrap();
    }

    #[test]
    fn core_platform_development_dependency_stays_local_across_normalization() {
        let release = BTreeMap::from([
            ("gpui".into(), "xgpui".into()),
            ("gpui-platform".into(), "xgpui-platform".into()),
        ]);
        let mut document = "[package]\nname='xgpui'\nversion='1.0.0'\n[dev-dependencies]\ngpui_platform={package='xgpui-platform',version='=1.0.0',features=['test-support']}\n"
            .parse::<DocumentMut>().unwrap();
        for _ in 0..2 {
            prepare_manifest(
                &mut document,
                &Names::new(),
                "1.25.3",
                &release,
                "1.0.0",
                &Names::new(),
            )
            .unwrap();
            let dependency = &document["dev-dependencies"]["gpui_platform"];
            assert_eq!(dependency["package"].as_str(), Some("xgpui-platform"));
            assert_eq!(dependency["path"].as_str(), Some("../gpui-platform"));
            assert!(dependency.get("version").is_none());
            assert_eq!(dependency["features"][0].as_str(), Some("test-support"));
        }
    }

    #[test]
    fn extension_versions_are_independent_of_framework_dependencies() {
        let release = BTreeMap::from([
            ("gpui".into(), "xgpui".into()),
            ("xgpui-color".into(), "xgpui-color".into()),
            ("xgpui-svg".into(), "xgpui-svg".into()),
        ]);
        let versions = BTreeMap::from([
            ("xgpui-color".into(), "0.2.0".into()),
            ("xgpui-svg".into(), "0.2.0".into()),
        ]);
        let mut manifest = "[package]\nname='xgpui-svg'\nversion='1.0.0'\n[dependencies]\ngpui={package='xgpui',version='=1.0.0',path='../gpui',features=['test-support']}\nxgpui-color={path='../xgpui-color',version='=1.0.0'}\n".parse::<DocumentMut>().unwrap();
        prepare_manifest(
            &mut manifest,
            &Names::new(),
            "1.25.3",
            &release,
            "1.0.0",
            &versions,
        )
        .unwrap();
        assert_eq!(manifest["package"]["version"].as_str(), Some("0.2.0"));
        assert_eq!(
            manifest["dependencies"]["gpui"]["version"].as_str(),
            Some("=1.0.0")
        );
        assert_eq!(
            manifest["dependencies"]["gpui"]["path"].as_str(),
            Some("../gpui")
        );
        assert_eq!(
            manifest["dependencies"]["xgpui-color"]["version"].as_str(),
            Some("=0.2.0")
        );
        let first = manifest.to_string();
        prepare_manifest(
            &mut manifest,
            &Names::new(),
            "1.25.3",
            &release,
            "1.0.0",
            &versions,
        )
        .unwrap();
        assert_eq!(manifest.to_string(), first);
    }

    #[test]
    fn unmapped_crate_and_directory_collision_do_not_mutate_capture() {
        for collision in [false, true] {
            let (dir, mut names) = fixture();
            let root = dir.path();
            let before = fs::read(root.join("upstream.json")).unwrap();
            if collision {
                fs::create_dir(root.join("gpui/crates/gpui")).unwrap();
            } else {
                names.remove("helper-gpui-unofficial");
            }
            assert!(normalize(root, &names, false).is_err());
            assert_eq!(before, fs::read(root.join("upstream.json")).unwrap());
            assert!(root.join("gpui/crates/gpui-unofficial/Cargo.toml").exists());
        }
    }
    #[test]
    fn next_capture_removes_old_crates_and_renormalizes_without_stale_patch_entries() {
        let (dir, names) = fixture();
        let root = dir.path();
        normalize(root, &names, false).unwrap();
        // A sync restores the complete raw capture before normalization, rather than overlaying it.
        fs::remove_dir_all(root.join("gpui")).unwrap();
        fs::create_dir_all(root.join("gpui/crates/gpui-unofficial")).unwrap();
        fs::write(
            root.join("gpui/crates/gpui-unofficial/Cargo.toml"),
            "[package]\nname='gpui-unofficial'\nversion='2.0.0'\n",
        )
        .unwrap();
        fs::write(root.join("upstream.json"),json!({"commit":"next-sha","packages":{"gpui-unofficial":"gpui/crates/gpui-unofficial"}}).to_string()).unwrap();
        normalize(root, &names, false).unwrap();
        let config = fs::read_to_string(root.join(".cargo/config.toml")).unwrap();
        assert!(!config.contains("helper"));
        normalize(root, &names, true).unwrap();
    }
    #[test]
    fn dependency_keys_features_and_lock_references_are_rewritten_but_urls_are_not() {
        let (_, names) = fixture();
        let mut doc="[dependencies]\ngpui-unofficial='1'\n[features]\nfeature=['dep:gpui-unofficial','gpui-unofficial?/feature']\n".parse::<DocumentMut>().unwrap();
        rewrite_item(doc.as_item_mut(), &names).unwrap();
        assert!(doc["dependencies"].get("gpui").is_some());
        assert_eq!(doc["features"]["feature"][0].as_str(), Some("dep:gpui"));
        assert_eq!(
            doc["features"]["feature"][1].as_str(),
            Some("gpui?/feature")
        );
        assert_eq!(rewrite("gpui-unofficial 1.0.0", &names), "gpui 1.0.0");
    }
    #[test]
    fn harness_uses_package_map_and_preserves_target_boundaries() {
        let (dir, mut names) = fixture();
        let root = dir.path();
        let mut provenance: Json =
            serde_json::from_str(&fs::read_to_string(root.join("upstream.json")).unwrap()).unwrap();
        for short in [
            "gpui-platform",
            "gpui-apple",
            "gpui-macos",
            "gpui-linux",
            "gpui-windows",
            "gpui-web",
        ] {
            let old = format!("{short}-gpui-unofficial");
            let path = format!("gpui/crates/{old}");
            fs::create_dir_all(root.join(&path)).unwrap();
            fs::write(
                root.join(&path).join("Cargo.toml"),
                format!("[package]\nname='{old}'\nversion='1.0.0'\n[workspace]\n"),
            )
            .unwrap();
            provenance["packages"][&old] = Json::String(path);
            names.insert(old, short.into());
        }
        fs::write(root.join("upstream.json"), provenance.to_string()).unwrap();
        fs::write(
            root.join(".cargo/validation.toml"),
            "[patch.crates-io]\nproptest={git='example',rev='revision'}\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("tools/build-all")).unwrap();
        fs::create_dir_all(root.join("gpui/crates/transformed-crates/extra")).unwrap();
        fs::write(
            root.join("gpui/crates/transformed-crates/extra/Cargo.toml"),
            "invalid staging manifest",
        )
        .unwrap();
        // Obsolete mirrors are excluded from validation and removed, not normalized.
        assert!(normalize(root, &names, true).is_err());
        normalize(root, &names, false).unwrap();
        assert!(!root.join("gpui/crates/transformed-crates").exists());
        normalize(root, &names, true).unwrap();
        build_manifests(root, false).unwrap();
        build_manifests(root, true).unwrap();
        let workspace: toml::Value =
            toml::from_str(&fs::read_to_string(root.join("Cargo.toml")).unwrap()).unwrap();
        assert_eq!(
            workspace["workspace"]["exclude"].as_array().unwrap().len(),
            8
        );
        assert!(workspace["patch"]["crates-io"].get("proptest").is_none());
        let harness: toml::Value =
            toml::from_str(&fs::read_to_string(root.join("tools/build-all/Cargo.toml")).unwrap())
                .unwrap();
        assert!(harness["dependencies"].get("gpui").is_some());
        assert!(harness["dependencies"]["gpui"].get("optional").is_none());
        assert_eq!(
            harness["dependencies"]["helper"]["optional"].as_bool(),
            Some(true)
        );
        let full = harness["features"]["full-capture"].as_array().unwrap();
        assert!(full.iter().any(|v| v.as_str() == Some("dep:helper")));
        assert!(!full.iter().any(|v| v.as_str() == Some("dep:gpui")));
        assert!(harness["dependencies"].get("gpui-linux").is_none());
        assert!(
            harness["target"]["cfg(target_os = \"macos\")"]["dependencies"]
                .get("gpui-macos")
                .is_some()
        );
        assert!(
            harness["target"]["cfg(target_family = \"wasm\")"]["dependencies"]
                .get("gpui-web")
                .is_some()
        );
        fs::write(root.join("Cargo.toml"), "stale").unwrap();
        assert!(build_manifests(root, true).is_err());
    }
}
