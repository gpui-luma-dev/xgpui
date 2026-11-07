use super::*;
pub(super) fn patch_version(version: &str) -> Result<String> {
    let parts = version
        .split('.')
        .map(str::parse)
        .collect::<std::result::Result<Vec<u64>, _>>()?;
    ensure!(
        parts.len() == 3,
        "Review prerelease version manually: {version}"
    );
    Ok(format!(
        "{}.{}.{}",
        parts[0],
        parts[1],
        parts[2].checked_add(1).context("Version overflow")?
    ))
}
fn prepare(root: &Path, baseline: Option<&str>) -> Result<Json> {
    let baseline = match baseline {
        Some(baseline) => baseline.to_owned(),
        None => git(
            root,
            &["describe", "--tags", "--match", "release-*", "--abbrev=0"],
        )?,
    };
    let base = git(root, &["rev-parse", &format!("{baseline}^{{commit}}")])?;
    git(root, &["merge-base", "--is-ancestor", &base, "HEAD"])?;
    let plan = inventory::plan(root)?;
    let changed: BTreeSet<_> = git(root, &["diff", "--name-only", &base, "HEAD"])?
        .lines()
        .map(str::to_owned)
        .collect();
    let mut selected = BTreeMap::new();
    let shared = [
        "scripts/release-packages.json",
        "upstream.json",
        "tools/release-sync/src/release/registry.rs",
        "tools/release-sync/src/release/inventory.rs",
    ]
    .iter()
    .any(|p| changed.contains(*p));
    let mut versions = BTreeMap::new();
    let mut documents = BTreeMap::new();
    for row in rows(&plan["packages"]) {
        let name = str_at(row, "name");
        versions.insert(name.to_owned(), patch_version(str_at(row, "version"))?);
        documents.insert(
            name.to_owned(),
            read_toml(root.join(str_at(row, "manifest")))?,
        );
        let prefix = format!(
            "{}/",
            Path::new(str_at(row, "manifest"))
                .parent()
                .unwrap()
                .display()
        );
        if shared {
            selected.insert(name.to_owned(), "shared release inputs changed");
        } else if changed
            .iter()
            .any(|p| p.starts_with(&prefix) && !p.ends_with("/Cargo.lock"))
        {
            selected.insert(name.to_owned(), "crate files changed");
        }
    }
    loop {
        let mut added = Vec::new();
        for row in rows(&plan["packages"]) {
            let name = str_at(row, "name");
            if selected.contains_key(name) {
                continue;
            }
            for d in inventory::dependencies(&documents[name]) {
                let dep = str_at(&d, "package");
                if selected.contains_key(dep)
                    && d.get("version").is_some()
                    && !internal_requirement_matches(str_at(&d, "version"), &versions[dep])?
                {
                    added.push(name.to_owned());
                    break;
                }
            }
        }
        if added.is_empty() {
            break;
        }
        for name in added {
            selected.insert(name, "dependency requirement changes");
        }
    }
    let rows:Vec<_>=rows(&plan["packages"]).iter().filter(|r|selected.contains_key(str_at(r,"name"))).map(|r|json!({"name":r["name"],"old":r["version"],"new":versions[str_at(r,"name")],"reason":selected[str_at(r,"name")]})).collect();
    Ok(
        json!({"baseline":baseline,"baseline_commit":base,"source_commit":git(root,&["rev-parse","HEAD"])?,"packages":rows}),
    )
}
fn rewrite_dependencies(item: &mut Item, versions: &Names) -> Result<()> {
    if let Some(table) = item.as_table_like_mut() {
        for (key, value) in table.iter_mut() {
            if matches!(
                key.get(),
                "dependencies" | "build-dependencies" | "dev-dependencies"
            ) {
                if let Some(deps) = value.as_table_like_mut() {
                    for (alias, dep) in deps.iter_mut() {
                        let name = dep
                            .get("package")
                            .and_then(Item::as_str)
                            .unwrap_or(alias.get())
                            .to_owned();
                        let Some(version) = versions.get(&name) else {
                            continue;
                        };
                        let current = dep
                            .as_str()
                            .or_else(|| dep.get("version").and_then(Item::as_str));
                        if let Some(current) = current {
                            if internal_requirement_matches(current, version)? {
                                continue;
                            }
                            let prefix = if current.starts_with('=') {
                                "="
                            } else if current.starts_with('~') {
                                "~"
                            } else if current.starts_with('^') {
                                "^"
                            } else {
                                ""
                            };
                            let new = format!("{prefix}{version}");
                            if dep.is_str() {
                                *dep = toml_edit::value(new);
                            } else {
                                dep["version"] = toml_edit::value(new);
                            }
                        }
                    }
                }
            } else {
                rewrite_dependencies(value, versions)?;
            }
        }
    }
    Ok(())
}
pub(super) fn rewrite_manifest(text: &str, versions: &Names) -> Result<String> {
    let mut doc = text.parse::<DocumentMut>()?;
    if let Some(name) = doc
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(Item::as_str)
        .map(str::to_owned)
        && let Some(v) = versions.get(&name)
    {
        doc["package"]["version"] = toml_edit::value(v.as_str());
    }
    rewrite_dependencies(doc.as_item_mut(), versions)?;
    Ok(doc.to_string())
}
fn external_locks(text: &str) -> Result<BTreeSet<String>> {
    let doc = parse_toml(text)?;
    rows(&doc["package"])
        .iter()
        .filter(|p| p.get("source").is_some())
        .map(|p| {
            Ok(serde_json::to_string(&json!([
                p["name"],
                p["version"],
                p["source"],
                p["checksum"]
            ]))?)
        })
        .collect()
}
pub(super) fn check_locks(root: &Path) -> Result<()> {
    for name in git(root, &["ls-files"])?
        .lines()
        .filter(|n| n.ends_with("Cargo.lock"))
    {
        let before = git(root, &["show", &format!("HEAD:{name}")])?;
        ensure!(
            external_locks(&before)? == external_locks(&fs::read_to_string(root.join(name))?)?,
            "External dependency resolution changed; review manually: {name}"
        );
    }
    Ok(())
}
fn apply(root: &Path, plan: &Json) -> Result<()> {
    ensure!(
        git(root, &["status", "--porcelain"])?.is_empty(),
        "Preparation requires a clean checkout"
    );
    let versions: Names = rows(&plan["packages"])
        .iter()
        .map(|r| (str_at(r, "name").into(), str_at(r, "new").into()))
        .collect();
    let path = root.join("scripts/release-packages.json");
    let mut map = read_json(&path)?;
    for (n, v) in &versions {
        map["versions"][n] = json!(v);
    }
    let mut writes = Vec::new();
    for name in git(root, &["ls-files"])?.lines() {
        let path = root.join(name);
        if name.ends_with("Cargo.toml") {
            writes.push((
                path.clone(),
                rewrite_manifest(&fs::read_to_string(&path)?, &versions)?,
            ));
        } else if name.ends_with("Cargo.lock") {
            let mut doc = fs::read_to_string(&path)?.parse::<DocumentMut>()?;
            for section in ["package", "patch.unused"] {
                let items = if section == "package" {
                    doc.get_mut("package")
                } else {
                    doc.get_mut("patch").and_then(|p| p.get_mut("unused"))
                };
                if let Some(items) = items.and_then(Item::as_array_of_tables_mut) {
                    for package in items.iter_mut() {
                        let name = package["name"].as_str().unwrap_or("").to_owned();
                        if package.get("source").is_none()
                            && let Some(v) = versions.get(&name)
                        {
                            package["version"] = toml_edit::value(v.as_str());
                        }
                    }
                }
            }
            let mut text = doc.to_string();
            for r in rows(&plan["packages"]) {
                text = text.replace(
                    &format!("\"{} {}\"", str_at(r, "name"), str_at(r, "old")),
                    &format!("\"{} {}\"", str_at(r, "name"), str_at(r, "new")),
                );
            }
            writes.push((path, text));
        }
    }
    write_json(path, &map)?;
    for (path, text) in writes {
        fs::write(path, text)?;
    }
    let names = read_names(&root.join("scripts/crate-names.json"))?;
    normalize(root, &names, false)?;
    build_manifests(root, false)?;
    Ok(())
}
fn refresh_locks(root: &Path, offline: bool) -> Result<()> {
    for name in git(root, &["ls-files"])?
        .lines()
        .filter(|n| n.ends_with("Cargo.lock"))
    {
        let text = fs::read_to_string(root.join(name))?;
        let mut command = Command::new("cargo");
        command
            .current_dir(root)
            .args(["metadata", "--format-version", "1", "--manifest-path"])
            .arg(Path::new(name).with_file_name("Cargo.toml"))
            .stdout(Stdio::null());
        if text.contains("github.com/proptest-rs/proptest")
            || text.contains("github.com/zed-industries/async-process")
        {
            command.args(["--config", ".cargo/validation.toml"]);
        }
        if offline {
            command.arg("--offline");
        }
        checked(&mut command)?;
    }
    check_locks(root)
}
fn notes(plan: &Json) -> String {
    let mut text = format!(
        "Daily release preparation\n\nBaseline: `{}` (`{}`).\n\n**Review required:** patch bumps are proposals. Review public API changes and choose minor/major versions before merging.\n\n| Crate | Old | Proposed | Reason |\n| --- | --- | --- | --- |\n",
        str_at(plan, "baseline"),
        str_at(plan, "baseline_commit")
    );
    for r in rows(&plan["packages"]) {
        text += &format!(
            "| {} | {} | {} | {} |\n",
            str_at(r, "name"),
            str_at(r, "old"),
            str_at(r, "new"),
            str_at(r, "reason")
        );
    }
    text
}
pub(super) fn metadata(report: &Json, date: &str) -> Result<(String, String, String)> {
    ensure!(
        report["mode"] == "publish"
            && report["status"] == "published-verified"
            && report["release_ready"] == true,
        "Finalization requires a successful verified publication report"
    );
    let sha = str_at(report, "source_commit");
    ensure!(
        sha.len() == 40
            && sha
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "Invalid source commit"
    );
    ensure!(
        !rows(&report["packages"]).is_empty()
            && rows(&report["packages"]).iter().all(|r| matches!(
                str_at(r, "status"),
                "already-published" | "published-verified"
            )),
        "Not all packages verified"
    );
    ensure!(
        date.len() >= 10 && date.as_bytes()[4] == b'-' && date.as_bytes()[7] == b'-',
        "Invalid source date"
    );
    let tag = format!("release-{}-{}", &date[..10], &sha[..12]);
    let mut notes = format!(
        "Verified crates.io release from `{sha}`.\n\n| Crate | Version | Result |\n| --- | --- | --- |\n"
    );
    for r in rows(&report["packages"]) {
        notes += &format!(
            "| `{}` | {} | {} |\n",
            str_at(r, "name"),
            str_at(r, "version"),
            str_at(r, "status")
        );
    }
    Ok((tag, sha.into(), notes))
}
fn finalize(root: &Path, report: &Json, repo: &str) -> Result<()> {
    let sha = str_at(report, "source_commit");
    ensure!(
        git(root, &["rev-parse", "HEAD"])? == sha,
        "Report does not match checked-out commit"
    );
    let date_output = Command::new("git")
        .current_dir(root)
        .env("TZ", "UTC")
        .args([
            "show",
            "-s",
            "--format=%cd",
            "--date=format-local:%Y-%m-%d",
            sha,
        ])
        .output()?;
    ensure!(date_output.status.success(), "Cannot read UTC source date");
    let date = String::from_utf8(date_output.stdout)?.trim().to_owned();
    let (tag, sha, notes) = metadata(report, &date)?;
    let remote = git(
        root,
        &[
            "ls-remote",
            "origin",
            &format!("refs/tags/{tag}"),
            &format!("refs/tags/{tag}^{{}}"),
        ],
    )?;
    if remote.is_empty() {
        checked(
            Command::new("gh")
                .args([
                    "api",
                    "--method",
                    "POST",
                    &format!("repos/{repo}/git/refs"),
                    "-f",
                    &format!("ref=refs/tags/{tag}"),
                    "-f",
                    &format!("sha={sha}"),
                ])
                .current_dir(root),
        )?;
    } else {
        let refs: BTreeMap<_, _> = remote
            .lines()
            .filter_map(|s| s.split_once('\t').map(|(sha, r)| (r, sha)))
            .collect();
        ensure!(
            refs.get(format!("refs/tags/{tag}^{{}}").as_str())
                .or_else(|| refs.get(format!("refs/tags/{tag}").as_str()))
                == Some(&sha.as_str()),
            "Existing release tag targets a different commit"
        );
    }
    let temp = tempfile::NamedTempFile::new()?;
    fs::write(temp.path(), notes)?;
    let existing = Command::new("gh")
        .args(["release", "view", &tag, "--repo", repo])
        .current_dir(root)
        .output()?
        .status
        .success();
    let mut cmd = Command::new("gh");
    cmd.current_dir(root)
        .args([
            "release",
            if existing { "edit" } else { "create" },
            &tag,
            "--repo",
            repo,
            "--title",
            &format!("XGPUI {}", tag.trim_start_matches("release-")),
            "--notes-file",
        ])
        .arg(temp.path());
    if !existing {
        cmd.arg("--verify-tag");
    }
    checked(&mut cmd)
}
pub(super) fn cli(root: &Path, o: &Options) -> Result<()> {
    match o.0.get(1).map(String::as_str) {
        Some("prepare") => {
            let report = outside(root, Path::new(o.required("--report")?))?;
            let notes_path = o
                .get("--notes")
                .map(|p| outside(root, Path::new(p)))
                .transpose()?;
            let plan = prepare(root, o.get("--baseline"))?;
            if o.has("--apply") {
                apply(root, &plan)?;
            }
            write_json(report, &plan)?;
            if let Some(path) = notes_path {
                fs::write(path, notes(&plan))?;
            }
            Ok(())
        }
        Some("refresh-locks") => refresh_locks(root, o.has("--offline")),
        Some("check-locks") => check_locks(root),
        Some("finalize") => finalize(
            root,
            &read_json(o.required("--report")?)?,
            o.required("--repo")?,
        ),
        _ => bail!("Usage: daily prepare|refresh-locks|check-locks|finalize [options]"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_rewrite_preserves_external_and_compatible_minimum() {
        let text = "[package]\nname='xgpui'\nversion='3.0.0'\n[dependencies]\ncore={package='xgpui',version='3.0.0'}\nexternal='3.0.0'\n[target.'cfg(windows)'.dev-dependencies]\nxgpui='=3.0.0'\n";
        let result =
            rewrite_manifest(text, &Names::from([("xgpui".into(), "3.0.1".into())])).unwrap();
        let doc = parse_toml(&result).unwrap();
        assert_eq!(doc["package"]["version"], "3.0.1");
        assert_eq!(doc["dependencies"]["core"]["version"], "3.0.0");
        assert_eq!(doc["dependencies"]["external"], "3.0.0");
        assert_eq!(
            doc["target"]["cfg(windows)"]["dev-dependencies"]["xgpui"],
            "=3.0.1"
        );
    }
    #[test]
    fn multiline_and_zero_major_boundaries() {
        let text = "[dependencies.gpui]\npackage='xgpui'\nversion='0.4.0'\n";
        let updated =
            rewrite_manifest(text, &Names::from([("xgpui".into(), "0.5.0".into())])).unwrap();
        assert_eq!(
            parse_toml(&updated).unwrap()["dependencies"]["gpui"]["version"],
            "0.5.0"
        );
    }
    #[test]
    fn external_lock_guard() {
        assert_ne!(
            external_locks(
                "[[package]]\nname='external'\nversion='1.0.0'\nsource='registry+test'\n"
            )
            .unwrap(),
            external_locks(
                "[[package]]\nname='external'\nversion='1.0.1'\nsource='registry+test'\n"
            )
            .unwrap()
        );
    }
    #[test]
    fn finalization_rejects_incomplete_reports() {
        let mut r = json!({"mode":"publish","status":"published-verified","release_ready":true,"source_commit":"a".repeat(40),"packages":[{"name":"xgpui","version":"3.0.0","status":"published-verified"}]});
        let (tag, _, _) = metadata(&r, "2026-11-07").unwrap();
        assert_eq!(tag, "release-2026-11-07-aaaaaaaaaaaa");
        r["release_ready"] = json!(false);
        assert!(metadata(&r, "2026-11-07").is_err());
    }
}
#[cfg(test)]
mod preparation_regressions {
    use super::super::test_fixture::{commit, repository};
    use super::*;
    #[test]
    fn compatible_dependency_does_not_cascade_daily_versions() {
        let fixture = repository();
        let root = fixture.path();
        fs::write(root.join("gpui/crates/gpui/src/lib.rs"), "changed").unwrap();
        commit(root);
        let plan = prepare(root, Some("release-epoch")).unwrap();
        assert_eq!(rows(&plan["packages"]).len(), 1);
        assert_eq!(plan["packages"][0]["name"], "xgpui");
        assert_eq!(plan["packages"][0]["new"], "1.0.1");
    }
    #[test]
    fn exact_dependency_cascades_when_proposed_patch_excludes_it() {
        let fixture = repository();
        let root = fixture.path();
        let web = root.join("gpui/crates/gpui-web/Cargo.toml");
        fs::write(
            &web,
            fs::read_to_string(&web)
                .unwrap()
                .replace("version='1.0.0',path", "version='=1.0.0',path"),
        )
        .unwrap();
        commit(root);
        git(root, &["tag", "-f", "release-epoch"]).unwrap();
        fs::write(root.join("gpui/crates/gpui/src/lib.rs"), "changed").unwrap();
        commit(root);
        let plan = prepare(root, Some("release-epoch")).unwrap();
        assert_eq!(rows(&plan["packages"]).len(), 2);
        assert_eq!(
            plan["packages"][1]["reason"],
            "dependency requirement changes"
        );
    }
}
