use super::*;
fn ancestor(root: &Path, older: &str, newer: &str) -> Result<bool> {
    let out = Command::new("git")
        .args(["merge-base", "--is-ancestor", older, newer])
        .current_dir(root)
        .output()?;
    ensure!(
        matches!(out.status.code(), Some(0 | 1)),
        "Cannot check ancestry: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(out.status.success())
}
fn history(repository: &str) -> Result<Vec<Json>> {
    let mut all = Vec::new();
    for page in 1.. {
        let batch = fetch_json(
            &format!("https://api.github.com/repos/{repository}/releases?per_page=100&page={page}"),
            false,
        )?
        .unwrap();
        let entries = batch
            .as_array()
            .context("Unexpected GitHub releases response")?;
        all.extend(entries.iter().filter(|r| r["draft"] != true).cloned());
        if entries.len() < 100 {
            break;
        }
    }
    Ok(all)
}
pub(super) fn verified_versions(body: &str) -> BTreeMap<String, String> {
    body.lines()
        .filter_map(|line| {
            let cells: Vec<_> = line
                .trim()
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim().trim_matches('`'))
                .collect();
            if cells.len() == 3 && matches!(cells[2], "already-published" | "published-verified") {
                Some((cells[0].into(), cells[1].into()))
            } else {
                None
            }
        })
        .collect()
}
fn manifests_at(root: &Path, revision: &str) -> Result<BTreeMap<String, String>> {
    inventory::manifests_from(
        &serde_json::from_str(&git(
            root,
            &["show", &format!("{revision}:scripts/release-packages.json")],
        )?)?,
        &serde_json::from_str(&git(root, &["show", &format!("{revision}:upstream.json")])?)?,
    )
}
fn changed_paths(root: &Path, revision: &str) -> Result<BTreeSet<String>> {
    let mut paths = BTreeSet::new();
    for args in [
        vec!["diff", "--no-renames", "--name-only", "-z", revision, "--"],
        vec!["ls-files", "--others", "--exclude-standard", "-z"],
    ] {
        paths.extend(
            git(root, &args)?
                .split('\0')
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
        );
    }
    Ok(paths)
}
fn crate_changes(
    root: &Path,
    manifest: &str,
    doc: &Json,
    changed: &BTreeSet<String>,
) -> Result<Vec<String>> {
    let dir = Path::new(manifest).parent().unwrap();
    let prefix = format!("{}/", dir.display());
    let mut selected: BTreeSet<String> = changed
        .iter()
        .filter(|p| p.starts_with(&prefix) && !p.ends_with("/Cargo.lock"))
        .cloned()
        .collect();
    for key in ["readme", "license-file", "build"] {
        if let Some(value) = doc["package"][key].as_str() {
            let path = absolute(&root.join(dir).join(value))?;
            ensure!(
                path.starts_with(root),
                "{manifest}: {key} is outside the repository"
            );
            let relative = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            if changed.contains(&relative) {
                selected.insert(relative);
            }
        }
    }
    Ok(selected.into_iter().collect())
}
fn sources(root: &Path, epoch: &str, releases: &[Json]) -> Result<(String, Vec<Json>)> {
    let base = git(
        root,
        &[
            "rev-parse",
            "--verify",
            &format!("refs/tags/{epoch}^{{commit}}"),
        ],
    )?;
    ensure!(
        ancestor(root, &base, "HEAD")?,
        "Epoch tag must be an ancestor of HEAD"
    );
    let ordered: BTreeMap<_, _> = git(root, &["rev-list", "--topo-order", "HEAD"])?
        .lines()
        .enumerate()
        .map(|(i, s)| (s.to_owned(), i))
        .collect();
    let mut candidates = Vec::new();
    for r in releases {
        let tag = str_at(r, "tag_name");
        if r["draft"] == true || tag == epoch || !tag.starts_with("release-") {
            continue;
        }
        let versions = verified_versions(str_at(r, "body"));
        if versions.is_empty() {
            continue;
        }
        let sha = git(
            root,
            &[
                "rev-parse",
                "--verify",
                &format!("refs/tags/{tag}^{{commit}}"),
            ],
        )
        .with_context(|| format!("Missing release tag {tag}; fetch origin --tags and retry"))?;
        if sha == base || !ordered.contains_key(&sha) || !ancestor(root, &base, &sha)? {
            continue;
        }
        candidates.push(json!({"tag":tag,"commit":sha,"versions":versions}));
    }
    candidates.sort_by_key(|r| ordered[str_at(r, "commit")]);
    Ok((base, candidates))
}
pub(super) fn plan_with_history(root: &Path, epoch: &str, releases: &[Json]) -> Result<Json> {
    let (base, sources) = sources(root, epoch, releases)?;
    let epoch_manifests = manifests_at(root, &base)?;
    let manifests = inventory::manifests(root)?;
    let mapping = read_json(root.join("scripts/release-packages.json"))?;
    let mut documents = BTreeMap::new();
    let mut diffs = BTreeMap::new();
    let mut rows_out = Vec::new();
    for (name, manifest) in &manifests {
        let doc = read_toml(root.join(manifest))?;
        let version = mapping["versions"][name]
            .as_str()
            .or(mapping["version"].as_str())
            .context("Missing package version")?;
        ensure!(
            doc["package"]["name"] == name.as_str() && doc["package"]["version"] == version,
            "{manifest}: name/version does not match release-packages.json"
        );
        let mut baseline = json!({"tag":epoch,"commit":base,"version":null,"manifest":null});
        if let Some(old) = epoch_manifests.get(name) {
            let old_doc = parse_toml(&git(root, &["show", &format!("{base}:{old}")])?)?;
            baseline["version"] = old_doc["package"]["version"].clone();
            baseline["manifest"] = json!(old);
        }
        for source in &sources {
            if let Some(verified) = source["versions"].get(name) {
                let commit = str_at(source, "commit");
                let old = manifests_at(root, commit)?;
                let path = old
                    .get(name)
                    .context("Verified crate missing from historical map")?;
                let old_doc = parse_toml(&git(root, &["show", &format!("{commit}:{path}")])?)?;
                ensure!(
                    &old_doc["package"]["version"] == verified,
                    "Verification version disagrees with tagged manifest"
                );
                baseline =
                    json!({"tag":source["tag"],"commit":commit,"version":verified,"manifest":path});
                break;
            }
        }
        let commit = str_at(&baseline, "commit").to_owned();
        if !diffs.contains_key(&commit) {
            diffs.insert(commit.clone(), changed_paths(root, &commit)?);
        }
        let files = crate_changes(root, manifest, &doc, &diffs[&commit])?;
        let mut reasons = Vec::new();
        if !files.is_empty() {
            reasons.push("crate files changed");
        }
        if baseline["version"].is_null() {
            reasons.push("new crate");
        }
        rows_out.push(json!({"name":name,"version":version,"manifest":manifest,"baseline":baseline,"changed_files":files,"reasons":reasons,"publish_policy":doc["package"].get("publish").unwrap_or(&json!(true))}));
        documents.insert(name.clone(), doc);
    }
    let versions: BTreeMap<_, _> = rows_out
        .iter()
        .map(|r| {
            (
                str_at(r, "name").to_owned(),
                str_at(r, "version").to_owned(),
            )
        })
        .collect();
    let mut incompatible = BTreeMap::<String, BTreeSet<String>>::new();
    for (name, doc) in &documents {
        let mut deps = BTreeSet::new();
        for d in inventory::dependencies(doc) {
            if let Some(version) = versions.get(str_at(&d, "package"))
                && d.get("version").is_some()
                && !internal_requirement_matches(str_at(&d, "version"), version)?
            {
                deps.insert(str_at(&d, "package").to_owned());
            }
        }
        incompatible.insert(name.clone(), deps);
    }
    let mut selected: BTreeSet<_> = rows_out
        .iter()
        .filter(|r| !rows(&r["reasons"]).is_empty())
        .map(|r| str_at(r, "name").to_owned())
        .collect();
    loop {
        let added: BTreeSet<_> = incompatible
            .iter()
            .filter(|(n, deps)| !selected.contains(*n) && !deps.is_disjoint(&selected))
            .map(|(n, _)| n.clone())
            .collect();
        if added.is_empty() {
            break;
        }
        selected.extend(added);
    }
    for r in &mut rows_out {
        let name = str_at(r, "name").to_owned();
        let changed: Vec<_> = incompatible[&name]
            .intersection(&selected)
            .cloned()
            .collect();
        if !changed.is_empty() {
            r["reasons"].as_array_mut().unwrap().push(json!(
                "internal dependency changes require version/pin review"
            ));
        }
        r["dependency_changes"] = json!(changed);
        r["changed"] = json!(selected.contains(&name));
        let mut updates = Vec::new();
        for d in inventory::dependencies(&documents[&name]) {
            let dep = str_at(&d, "package");
            if selected.contains(dep)
                && d.get("version").is_some()
                && !internal_requirement_matches(str_at(&d, "version"), &versions[dep])?
            {
                updates.push(
                    json!({"dependency":dep,"current":d["version"],"required":versions[dep]}),
                );
            }
        }
        r["pin_updates"] = json!(updates);
    }
    Ok(json!({"epoch":epoch,"epoch_commit":base,"packages":rows_out}))
}
pub(super) fn identity(root: &Path) -> Result<Json> {
    let commit = git(root, &["rev-parse", "HEAD"])?;
    let paths: BTreeSet<_> = git(
        root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?
    .split('\0')
    .filter(|s| !s.is_empty())
    .map(str::to_owned)
    .collect();
    let mut digest = Sha256::new();
    for name in paths {
        let path = root.join(&name);
        let (kind, contents) = if path.is_symlink() {
            (
                "symlink",
                fs::read_link(&path)?.to_string_lossy().as_bytes().to_vec(),
            )
        } else if path.is_file() {
            let executable = {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::metadata(&path)?.permissions().mode() & 0o111 != 0
                }
                #[cfg(not(unix))]
                {
                    false
                }
            };
            (
                if executable { "executable" } else { "file" },
                fs::read(&path)?,
            )
        } else {
            ensure!(
                !path.exists(),
                "Unsupported source input {}",
                path.display()
            );
            ("deleted", Vec::new())
        };
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update(kind.as_bytes());
        digest.update([0]);
        digest.update(Sha256::digest(&contents));
    }
    Ok(
        json!({"commit":commit,"dirty":!git(root,&["status","--porcelain","--untracked-files=all"])?.is_empty(),"sha256":format!("{:x}",digest.finalize()),"scope":"tracked and non-ignored untracked file contents, paths and executable modes; excludes ignored inputs"}),
    )
}
fn report(root: &Path, epoch: Option<&str>) -> Result<Json> {
    let identity = identity(root)?;
    let config = read_json(root.join("scripts/release-check.json"))?;
    let mut plan = plan_with_history(
        root,
        epoch.unwrap_or(str_at(&config, "epoch")),
        &history(str_at(&config, "repository"))?,
    )?;
    for row in plan["packages"].as_array_mut().unwrap() {
        let result = (|| -> Result<()> {
            let name = str_at(row, "name").to_owned();
            let version = str_at(row, "version").to_owned();
            let doc = fetch_json(&format!("https://crates.io/api/v1/crates/{name}"), true)?;
            let mut versions = BTreeMap::new();
            let mut latest = Json::Null;
            if let Some(doc) = doc {
                ensure!(doc["crate"]["id"] == name, "Unexpected registry identity");
                latest = doc["crate"]["max_version"].clone();
                for v in rows(&doc["versions"]) {
                    versions.insert(str_at(v, "num").to_owned(), v.clone());
                }
            }
            row["registry_latest"] = latest;
            if let Some(baseline) = row["baseline"]["version"].as_str() {
                ensure!(
                    versions.contains_key(baseline),
                    "Baseline version {baseline} is not on crates.io; review release history"
                );
            }
            let existing = versions.get(&version);
            if let Some(v) = existing {
                row["yanked"] = v["yanked"].clone();
            }
            let status = if row["changed"] == true {
                if existing.is_some() {
                    "version-bump-required"
                } else if !allowed(&row["publish_policy"]) {
                    "publication-disabled"
                } else if !rows(&row["pin_updates"]).is_empty() {
                    "dependency-pin-update-required"
                } else {
                    "ready-for-new-version"
                }
            } else {
                ensure!(
                    existing.is_some(),
                    "Unchanged baseline version missing from crates.io"
                );
                "unchanged"
            };
            row["status"] = json!(status);
            row["action"] = json!(match status {
                "version-bump-required" =>
                    "Changed packaged content has an already published version; bump before release.",
                "publication-disabled" => "Enable crates.io publication before uploading.",
                "dependency-pin-update-required" =>
                    "Update incompatible internal requirements before release.",
                "ready-for-new-version" =>
                    "Changes have an unpublished version for release preparation.",
                _ => "No crate changes to publish.",
            });
            Ok(())
        })();
        if let Err(e) = result {
            row["status"] = json!("check-failed");
            row["error"] = json!(format!("{e:#}"));
        }
    }
    let errors = if self::identity(root)? != identity {
        vec!["Source changed during the check; rerun."]
    } else {
        vec![]
    };
    let mut counts = BTreeMap::new();
    for status in [
        "unchanged",
        "version-bump-required",
        "ready-for-new-version",
        "dependency-pin-update-required",
        "publication-disabled",
        "check-failed",
    ] {
        counts.insert(
            status,
            rows(&plan["packages"])
                .iter()
                .filter(|r| r["status"] == status)
                .count(),
        );
    }
    let passed = errors.is_empty()
        && [
            "version-bump-required",
            "dependency-pin-update-required",
            "publication-disabled",
            "check-failed",
        ]
        .iter()
        .all(|s| counts[s] == 0);
    Ok(
        json!({"schema_version":2,"generated_at":now(),"source":identity,"epoch":plan["epoch"],"epoch_commit":plan["epoch_commit"],"scope":"Conservative crate directory changes and internal dependency closure since verified release tags; no builds or archive payload comparison","uploads_enabled":false,"release_ready":false,"version_preflight_passed":passed,"counts":counts,"errors":errors,"packages":plan["packages"]}),
    )
}
pub(super) fn cli(root: &Path, o: &Options) -> Result<()> {
    let path = o
        .get("--report")
        .map(|p| outside(root, Path::new(p)))
        .transpose()?;
    let report = report(root, o.get("--epoch"))?;
    if let Some(path) = path {
        write_json(path, &report)?;
    }
    if o.has("--json") {
        print_json(&report)?
    } else {
        println!(
            "Crate release preflight (read-only)\nEpoch: {}",
            str_at(&report, "epoch")
        );
        for r in rows(&report["packages"]) {
            if r["status"] != "unchanged" {
                println!(
                    "[{}] {} {}: {}",
                    str_at(r, "status"),
                    str_at(r, "name"),
                    str_at(r, "version"),
                    r["error"].as_str().unwrap_or(str_at(r, "action"))
                );
                for p in rows(&r["pin_updates"]) {
                    println!(
                        "  Requirement {}: {} -> {}",
                        str_at(p, "dependency"),
                        str_at(p, "current"),
                        str_at(p, "required")
                    );
                }
            }
        }
        println!("{}", report["counts"]);
    }
    ensure!(
        report["version_preflight_passed"] == true,
        "Release preflight failed"
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verified_rows_only() {
        let v = verified_versions(
            "| `a` | 1.0.0 | published-verified |\n| b | 2.0.0 | uploaded |\n| c | 3.0.0 | already-published |",
        );
        assert_eq!(v.len(), 2);
        assert_eq!(v["a"], "1.0.0");
    }
    #[test]
    fn fingerprint_tracks_content_untracked_and_deletion() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        git(r, &["init", "-q"]).unwrap();
        git(r, &["config", "user.email", "fixture@example.test"]).unwrap();
        git(r, &["config", "user.name", "Fixture"]).unwrap();
        fs::write(r.join("a"), "one").unwrap();
        git(r, &["add", "a"]).unwrap();
        git(r, &["commit", "-qm", "base"]).unwrap();
        let base = identity(r).unwrap();
        fs::write(r.join("a"), "two").unwrap();
        assert_ne!(base["sha256"], identity(r).unwrap()["sha256"]);
        fs::write(r.join("a"), "one").unwrap();
        assert_eq!(base, identity(r).unwrap());
        fs::write(r.join("new"), "x").unwrap();
        assert_ne!(base, identity(r).unwrap());
        fs::remove_file(r.join("a")).unwrap();
        assert!(identity(r).is_ok());
    }
}

#[cfg(test)]
mod policy_regressions {
    use super::super::test_fixture::{bump, commit, repository};
    use super::*;
    fn row<'a>(plan: &'a Json, name: &str) -> &'a Json {
        rows(&plan["packages"])
            .iter()
            .find(|r| r["name"] == name)
            .unwrap()
    }
    #[test]
    fn compatible_release_does_not_propagate_to_dependents() {
        let fixture = repository();
        let root = fixture.path();
        fs::write(root.join("gpui/crates/gpui/src/lib.rs"), "changed").unwrap();
        bump(root, "xgpui", "1.0.1");
        let p = plan_with_history(root, "release-epoch", &[]).unwrap();
        assert_eq!(row(&p, "xgpui")["changed"], true);
        assert_eq!(row(&p, "xgpui-web")["changed"], false);
        assert!(rows(&row(&p, "xgpui-web")["pin_updates"]).is_empty());
    }
    #[test]
    fn exact_and_versioned_development_edges_reject_incompatible_candidate() {
        let fixture = repository();
        let root = fixture.path();
        let path = root.join("gpui/crates/gpui-web/Cargo.toml");
        fs::write(
            &path,
            fs::read_to_string(&path)
                .unwrap()
                .replace("version='1.0.0',path", "version='=1.0.0',path"),
        )
        .unwrap();
        let color = root.join("extensions/crates/xgpui-color/Cargo.toml");
        fs::write(&color,format!("{}[dev-dependencies]\ncore={{package='xgpui',version='=1.0.0',path='../../../gpui/crates/gpui'}}\n",fs::read_to_string(&color).unwrap())).unwrap();
        commit(root);
        git(root, &["tag", "-f", "release-epoch"]).unwrap();
        bump(root, "xgpui", "1.0.1");
        let p = plan_with_history(root, "release-epoch", &[]).unwrap();
        for name in ["xgpui-web", "xgpui-color"] {
            assert_eq!(row(&p, name)["changed"], true);
            assert_eq!(row(&p, name)["dependency_changes"], json!(["xgpui"]));
            assert_eq!(row(&p, name)["pin_updates"][0]["required"], "1.0.1");
        }
    }
    #[test]
    fn path_only_development_dependency_does_not_require_release() {
        let fixture = repository();
        let root = fixture.path();
        let color = root.join("extensions/crates/xgpui-color/Cargo.toml");
        fs::write(
            &color,
            format!(
                "{}[dev-dependencies]\ncore={{package='xgpui',path='../../../gpui/crates/gpui'}}\n",
                fs::read_to_string(&color).unwrap()
            ),
        )
        .unwrap();
        commit(root);
        git(root, &["tag", "-f", "release-epoch"]).unwrap();
        bump(root, "xgpui", "2.0.0");
        let p = plan_with_history(root, "release-epoch", &[]).unwrap();
        assert_eq!(row(&p, "xgpui-color")["changed"], false);
    }
    #[test]
    fn verified_release_resets_only_its_crate_baseline_and_drafts_do_not() {
        let fixture = repository();
        let root = fixture.path();
        bump(root, "xgpui-color", "1.0.1");
        commit(root);
        git(root, &["tag", "release-new"]).unwrap();
        let release = json!({"tag_name":"release-new","draft":false,"body":"| xgpui-color | 1.0.1 | published-verified |"});
        let p = plan_with_history(root, "release-epoch", std::slice::from_ref(&release)).unwrap();
        assert_eq!(row(&p, "xgpui-color")["changed"], false);
        assert_eq!(row(&p, "xgpui-color")["baseline"]["tag"], "release-new");
        let mut draft = release;
        draft["draft"] = json!(true);
        let p = plan_with_history(root, "release-epoch", &[draft]).unwrap();
        assert_eq!(row(&p, "xgpui-color")["changed"], true);
    }
    #[test]
    fn missing_release_tag_and_inconsistent_verified_version_fail_closed() {
        let fixture = repository();
        let root = fixture.path();
        let mut release = json!({"tag_name":"release-missing","draft":false,"body":"| xgpui | 9.0.0 | published-verified |"});
        assert!(plan_with_history(root, "release-epoch", &[release.clone()]).is_err());
        git(root, &["tag", "release-bad"]).unwrap(); // same-epoch tags do not override the baseline
        fs::write(root.join("README.md"), "new").unwrap();
        commit(root);
        git(root, &["tag", "-f", "release-bad"]).unwrap();
        release["tag_name"] = json!("release-bad");
        assert!(plan_with_history(root, "release-epoch", &[release]).is_err());
    }
    #[test]
    fn crate_external_assets_and_untracked_inputs_count_but_root_docs_do_not() {
        let fixture = repository();
        let root = fixture.path();
        fs::write(root.join("README.md"), "docs").unwrap();
        let p = plan_with_history(root, "release-epoch", &[]).unwrap();
        assert!(rows(&p["packages"]).iter().all(|r| r["changed"] == false));
        fs::write(root.join("gpui/crates/gpui/src/new.rs"), "new").unwrap();
        let p = plan_with_history(root, "release-epoch", &[]).unwrap();
        assert_eq!(
            row(&p, "xgpui")["changed_files"],
            json!(["gpui/crates/gpui/src/new.rs"])
        );
    }
}
