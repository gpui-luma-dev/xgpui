use super::*;
pub(super) fn dependencies(document: &Json) -> Vec<Json> {
    let mut result = Vec::new();
    let mut tables = vec![(None, document)];
    if let Some(targets) = document["target"].as_object() {
        tables.extend(targets.iter().map(|(k, v)| (Some(k.as_str()), v)));
    }
    for (target, table) in tables {
        for kind in ["dependencies", "build-dependencies", "dev-dependencies"] {
            if let Some(deps) = table[kind].as_object() {
                for (alias, value) in deps {
                    let spec = if value.is_string() {
                        json!({"version":value})
                    } else {
                        value.clone()
                    };
                    let mut dep = json!({"alias":alias,"package":spec["package"].as_str().unwrap_or(alias),"kind":kind,"target":target,"optional":spec["optional"].as_bool().unwrap_or(false)});
                    for key in ["version", "path", "git", "rev", "registry", "workspace"] {
                        if let Some(v) = spec.get(key) {
                            dep[key] = v.clone();
                        }
                    }
                    result.push(dep);
                }
            }
        }
    }
    result
}
pub(super) fn layers(
    graph: &BTreeMap<String, BTreeSet<String>>,
) -> (Vec<Vec<String>>, Vec<String>) {
    let mut pending = graph.clone();
    let mut layers = Vec::new();
    while !pending.is_empty() {
        let ready: Vec<_> = pending
            .iter()
            .filter(|(_, v)| v.is_empty())
            .map(|(k, _)| k.clone())
            .collect();
        if ready.is_empty() {
            break;
        }
        for key in &ready {
            pending.remove(key);
        }
        for deps in pending.values_mut() {
            for key in &ready {
                deps.remove(key);
            }
        }
        layers.push(ready);
    }
    (layers, pending.into_keys().collect())
}
pub(super) fn manifests(root: &Path) -> Result<BTreeMap<String, String>> {
    manifests_from(
        &read_json(root.join("scripts/release-packages.json"))?,
        &read_json(root.join("upstream.json"))?,
    )
}
pub(super) fn manifests_from(release: &Json, upstream: &Json) -> Result<BTreeMap<String, String>> {
    Ok(release["packages"]
        .as_object()
        .context("Missing release package map")?
        .iter()
        .map(|(local, name)| {
            (
                name.as_str().unwrap().to_owned(),
                format!(
                    "{}/Cargo.toml",
                    upstream["packages"][local]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("extensions/crates/{local}"))
                ),
            )
        })
        .collect())
}
pub(super) fn audit(root: &Path) -> Result<Json> {
    let release = read_json(root.join("scripts/release-packages.json"))?;
    let manifests = manifests(root)?;
    let mut packages = Vec::new();
    let mut issues = Vec::new();
    let mut graph: BTreeMap<String, BTreeSet<String>> = manifests
        .keys()
        .map(|n| (n.clone(), BTreeSet::new()))
        .collect();
    let mut dev = graph.clone();
    let version = |name: &str| {
        release["versions"][name]
            .as_str()
            .or(release["version"].as_str())
            .unwrap_or("")
    };
    for (name, manifest) in &manifests {
        let path = root.join(manifest);
        let doc = read_toml(&path)?;
        let p = &doc["package"];
        let deps = dependencies(&doc);
        let mut problems = Vec::<String>::new();
        if p["name"] != name.as_str() || p["version"] != version(name) {
            problems.push("release-map-identity-mismatch".into());
        }
        if !allowed(p.get("publish").unwrap_or(&json!(true))) {
            problems.push("publishing-disabled".into());
        }
        for key in ["description", "repository"] {
            if str_at(p, key).is_empty() {
                problems.push(format!("missing-{key}"));
            }
        }
        if str_at(p, "license").is_empty() && str_at(p, "license-file").is_empty() {
            problems.push("missing-license".into());
        }
        for d in &deps {
            let context = format!(
                "{}:{}:{}",
                str_at(d, "kind"),
                d["target"].as_str().unwrap_or("all"),
                str_at(d, "alias")
            );
            let local_dev = d["kind"] == "dev-dependencies"
                && d.get("path").is_some()
                && ["version", "git", "registry"]
                    .iter()
                    .all(|k| d.get(k).is_none());
            if d["workspace"] == true {
                problems.push(format!("unresolved-workspace-dependency:{context}"));
            }
            if (d.get("path").is_some() || d.get("git").is_some())
                && d.get("version").is_none()
                && !local_dev
            {
                problems.push(format!("dependency-without-registry-version:{context}"));
            }
            if d.get("git").is_some() {
                problems.push(format!(
                    "git-source-needs-registry-equivalence-review:{context}"
                ));
            }
            let target = str_at(d, "package");
            if manifests.contains_key(target) {
                dev.get_mut(name).unwrap().insert(target.into());
                if d["kind"] != "dev-dependencies" {
                    graph.get_mut(name).unwrap().insert(target.into());
                }
                if !local_dev
                    && !internal_requirement_matches(str_at(d, "version"), version(target))?
                {
                    problems.push(format!("internal-version-mismatch:{context}"));
                }
                if let Some(local) = d["path"].as_str()
                    && absolute(&path.parent().unwrap().join(local).join("Cargo.toml"))?
                        != absolute(&root.join(&manifests[target]))?
                {
                    problems.push(format!("internal-path-mismatch:{context}"));
                }
            }
        }
        issues.extend(problems.iter().map(|p| json!({"package":name,"issue":p})));
        packages.push(json!({"name":name,"version":p["version"],"manifest":manifest,"publish":p.get("publish").unwrap_or(&json!(true)),"metadata_review":(["homepage","readme"].iter().filter(|k|str_at(p,k).is_empty()).collect::<Vec<_>>()),"dependencies":deps,"issues":problems}));
    }
    let mut patches = Vec::new();
    for file in ["Cargo.toml", ".cargo/config.toml", ".cargo/validation.toml"] {
        if !root.join(file).exists() {
            continue;
        }
        let doc = read_toml(root.join(file))?;
        if let Some(registries) = doc["patch"].as_object() {
            for (registry, entries) in registries {
                if let Some(entries) = entries.as_object() {
                    for (name, spec) in entries {
                        patches.push(
                            json!({"file":file,"registry":registry,"package":name,"spec":spec}),
                        );
                    }
                }
            }
        }
    }
    let (layers, blocked) = layers(&graph);
    let (_, dev_blocked) = self::layers(&dev);
    Ok(
        json!({"schema_version":1,"scope":"manifest-only, all targets and optional edges; no registry resolution or build verification","release_ready":false,"packages":packages,"dependency_first_layers":layers,"cycle_blocked_packages":blocked,"including_dev_cycle_blocked_packages":dev_blocked,"patches_to_review_outside_repository":patches,"issues":issues,"remaining_gates":["registry name/version availability and authorized ownership", "prerequisites #18, #47 and #48 with native platform evidence", "package contents, assets and license attribution", "isolated package and consumer builds without repository patches", "Windows, Linux and macOS validation", "explicit first-publication authorization"]}),
    )
}
pub(super) fn plan(root: &Path) -> Result<Json> {
    let audit = audit(root)?;
    let mapping = read_json(root.join("scripts/release-packages.json"))?;
    let framework: BTreeSet<String> = mapping["packages"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(local, _)| !local.starts_with("xgpui-"))
        .map(|(_, v)| v.as_str().unwrap().into())
        .collect();
    ensure!(
        framework.contains("xgpui"),
        "Expected xgpui as the framework core"
    );
    let names: BTreeSet<_> = rows(&audit["packages"])
        .iter()
        .map(|r| str_at(r, "name").to_owned())
        .collect();
    let extensions: BTreeSet<_> = names.difference(&framework).cloned().collect();
    let mut graph = BTreeMap::new();
    for r in rows(&audit["packages"]) {
        let name = str_at(r, "name").to_owned();
        let deps: BTreeSet<String> = rows(&r["dependencies"])
            .iter()
            .filter(|d| d["kind"] != "dev-dependencies" && names.contains(str_at(d, "package")))
            .map(|d| str_at(d, "package").into())
            .collect();
        ensure!(
            !framework.contains(&name) || deps.is_disjoint(&extensions),
            "Framework depends on a child extension"
        );
        graph.insert(name, deps);
        for issue in rows(&r["issues"]) {
            let s = issue.as_str().unwrap();
            ensure!(
                ![
                    "release-map-identity-mismatch",
                    "internal-version-mismatch",
                    "internal-path-mismatch",
                    "unresolved-workspace-dependency"
                ]
                .iter()
                .any(|p| s.starts_with(p)),
                "{}: {s}",
                str_at(r, "name")
            );
        }
    }
    let mut packages = Vec::new();
    let mut phases = Vec::new();
    for (phase, set) in [("framework", framework), ("extensions", extensions)] {
        let sub = set
            .iter()
            .map(|n| (n.clone(), graph[n].intersection(&set).cloned().collect()))
            .collect();
        let (layers, blocked) = layers(&sub);
        ensure!(blocked.is_empty(), "{phase} dependency cycle: {blocked:?}");
        let ordered: Vec<_> = layers.into_iter().flatten().collect();
        for name in &ordered {
            let r = rows(&audit["packages"])
                .iter()
                .find(|r| r["name"] == name.as_str())
                .unwrap();
            packages.push(json!({"name":name,"version":r["version"],"manifest":r["manifest"],"role":if name=="xgpui" {"core"}else{phase.trim_end_matches('s')},"dependencies":graph[name],"publish_policy":r["publish"],"review_items":r["issues"]}));
        }
        phases.push(json!({"name":phase,"packages":ordered}));
    }
    cold_publication_order(&audit, &packages)?;
    Ok(
        json!({"schema_version":1,"registry_access":false,"uploads_enabled":false,"release_ready":false,"core_package":"xgpui","phases":phases,"packages":packages,"development_cycles":audit["including_dev_cycle_blocked_packages"],"patches_to_review":audit["patches_to_review_outside_repository"],"next_registry_phase":["Present exact names, versions, ownership, scope, commands and network activity for approval.", "After approval, check availability/ownership and resolve registry dependencies without checkout patches.", "Verify registry-equivalent packaging, including development cycles and Git dependency fallbacks.", "Upload in framework-first dependency order; confirm registry visibility before each dependent.", "Resume only the same commit after comparing already-published archive contents; stop on mismatch.", "Verify downloaded content and version-only consumers before calling a release complete."],"approval_required_before":["any crates.io API/index/download request, including availability checks", "registry-backed cargo package or cargo publish --dry-run", "credentials/ownership setup or enabling extension publication", "any upload, first registration, tag or GitHub release"]}),
    )
}
/// Registry manifests keep every versioned dependency, development edges
/// included, and each upload must resolve against what is already published.
/// Assume no candidate version exists yet: every versioned internal edge, on
/// any target and of any kind, must point at a package uploaded earlier. Path-only
/// development edges are omitted from archives and do not constrain order.
/// Staging all candidates together (as archive verification does) or relying on
/// an older published version would hide a violation, so check the plan itself.
fn cold_publication_order(audit: &Json, ordered: &[Json]) -> Result<()> {
    let position: BTreeMap<&str, usize> = ordered
        .iter()
        .enumerate()
        .map(|(i, r)| (str_at(r, "name"), i))
        .collect();
    let mut problems = Vec::new();
    for r in rows(&audit["packages"]) {
        let name = str_at(r, "name");
        let Some(&at) = position.get(name) else {
            continue;
        };
        for d in rows(&r["dependencies"]) {
            let target = str_at(d, "package");
            let Some(&needed) = position.get(target) else {
                continue;
            };
            if d.get("version").is_none() || needed < at {
                continue;
            }
            problems.push(format!(
                "{name} {}:{}:{} requires unpublished {target}",
                str_at(d, "kind"),
                d["target"].as_str().unwrap_or("all"),
                str_at(d, "alias")
            ));
        }
    }
    ensure!(
        problems.is_empty(),
        "Versioned edges break cold publication order (localize test-only reverse edges): {}",
        problems.join("; ")
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cycles_and_layers() {
        let g = BTreeMap::from([
            ("a".into(), BTreeSet::from(["b".into()])),
            ("b".into(), BTreeSet::from(["a".into()])),
            ("leaf".into(), BTreeSet::new()),
        ]);
        let (l, b) = layers(&g);
        assert_eq!(l, vec![vec!["leaf"]]);
        assert_eq!(b, vec!["a", "b"]);
    }
    #[test]
    fn target_optional_dev_edges() {
        let doc = json!({"target":{"cfg(windows)":{"dev-dependencies":{"core":{"package":"xgpui","version":"3.0.0","optional":true}}}}});
        let d = dependencies(&doc);
        assert_eq!(d[0]["package"], "xgpui");
        assert_eq!(d[0]["target"], "cfg(windows)");
        assert_eq!(d[0]["kind"], "dev-dependencies");
        assert_eq!(d[0]["optional"], true);
    }
}
#[cfg(test)]
mod policy_regressions {
    use super::super::test_fixture::repository;
    use super::*;
    #[test]
    fn framework_first_plan_and_dev_cycles() {
        let fixture = repository();
        let root = fixture.path();
        let plan = plan(root).unwrap();
        assert_eq!(plan["packages"][0]["name"], "xgpui");
        assert_eq!(plan["packages"][1]["name"], "xgpui-web");
        assert_eq!(plan["packages"][2]["name"], "xgpui-color");
        assert_eq!(plan["uploads_enabled"], false);
        assert_eq!(plan["release_ready"], false);
        let core = root.join("gpui/crates/gpui/Cargo.toml");
        fs::write(
            &core,
            format!(
                "{}[dev-dependencies]\nweb={{package='xgpui-web',path='../gpui-web'}}\n",
                fs::read_to_string(&core).unwrap()
            ),
        )
        .unwrap();
        let audit = audit(root).unwrap();
        assert!(rows(&audit["cycle_blocked_packages"]).is_empty());
        assert_eq!(
            audit["including_dev_cycle_blocked_packages"],
            json!(["xgpui", "xgpui-web"])
        );
    }
    #[test]
    fn versioned_reverse_development_edge_blocks_cold_publication() {
        // Neither candidate is published: core's versioned Wasm-only dev edge
        // on web needs web first, while web needs core first.
        let fixture = repository();
        let root = fixture.path();
        let core = root.join("gpui/crates/gpui/Cargo.toml");
        let original = fs::read_to_string(&core).unwrap();
        fs::write(
            &core,
            format!(
                "{original}[target.'cfg(target_family = \"wasm\")'.dev-dependencies]\ngpui_web={{package='xgpui-web',version='1.0.0',path='../gpui-web'}}\n"
            ),
        )
        .unwrap();
        let error = format!("{:#}", plan(root).unwrap_err());
        assert!(error.contains("cold publication"), "{error}");
        assert!(
            error.contains("xgpui dev-dependencies:cfg(target_family = \"wasm\"):gpui_web requires unpublished xgpui-web"),
            "{error}"
        );
        // The normalized (path-only) edge is omitted from the archive.
        fs::write(
            &core,
            format!(
                "{original}[target.'cfg(target_family = \"wasm\")'.dev-dependencies]\ngpui_web={{package='xgpui-web',path='../gpui-web'}}\n"
            ),
        )
        .unwrap();
        plan(root).unwrap();
    }
    #[test]
    fn versioned_development_edge_on_earlier_candidate_is_allowed() {
        let fixture = repository();
        let root = fixture.path();
        let color = root.join("extensions/crates/xgpui-color/Cargo.toml");
        fs::write(
            &color,
            format!(
                "{}[target.'cfg(windows)'.dev-dependencies]\nweb={{package='xgpui-web',version='1.0.0',path='../../../gpui/crates/gpui-web'}}\n",
                fs::read_to_string(&color).unwrap()
            ),
        )
        .unwrap();
        plan(root).unwrap();
    }
    #[test]
    fn bad_internal_candidate_and_framework_extension_edges_stop_plan() {
        let fixture = repository();
        let root = fixture.path();
        let web = root.join("gpui/crates/gpui-web/Cargo.toml");
        fs::write(
            &web,
            fs::read_to_string(&web)
                .unwrap()
                .replace("version='1.0.0',path", "version='=0.9.0',path"),
        )
        .unwrap();
        assert!(plan(root).is_err());
        let fixture = repository();
        let root = fixture.path();
        let web = root.join("gpui/crates/gpui-web/Cargo.toml");
        fs::write(
            &web,
            format!(
                "{}color={{package='xgpui-color',version='1.0.0'}}\n",
                fs::read_to_string(&web).unwrap()
            ),
        )
        .unwrap();
        assert!(plan(root).is_err());
    }
}
