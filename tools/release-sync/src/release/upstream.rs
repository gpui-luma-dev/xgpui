use super::*;
const SOURCE: &str = "https://github.com/gpui-archipelago/gpui-unofficial.git";
const SCOPES: [&str; 3] = ["gpui", ".cargo/config.toml", "upstream.json"];
const STAGING: &str = "gpui/crates/transformed-crates/";
fn full_sha(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|c| c.is_ascii_hexdigit())
}
fn stable_version(v: &str) -> bool {
    let p: Vec<_> = v.split('.').collect();
    p.len() == 3
        && p.iter()
            .all(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
}
// Reviewed proposals land on a branch whose PR is reviewed before merging.
// Epic subtask branches (`issue-N/task`) qualify; the epic itself and main
// only receive merged PRs, so proposals are never applied there directly.
fn review_branch(branch: &str) -> bool {
    fn named(rest: &str) -> bool {
        !rest.is_empty()
            && rest
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'/'))
            && rest
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..")
    }
    if let Some(rest) = branch
        .strip_prefix("sync/")
        .or_else(|| branch.strip_prefix("codex/"))
    {
        return named(rest);
    }
    let Some((issue, task)) = branch
        .strip_prefix("issue-")
        .and_then(|rest| rest.split_once('/'))
    else {
        return false;
    };
    !issue.is_empty() && issue.bytes().all(|c| c.is_ascii_digit()) && task != "epic" && named(task)
}
fn safe_link(path: &Path, link: &Path) -> Result<()> {
    let mut target = path.parent().context("Missing link parent")?.to_path_buf();
    for component in link.components() {
        match component {
            Component::Normal(part) => target.push(part),
            Component::CurDir => (),
            Component::ParentDir => ensure!(target.pop(), "Symlink escapes snapshot"),
            _ => bail!("Absolute symlink target in snapshot"),
        }
    }
    Ok(())
}
// Snapshot imports preserve lockfiles and configuration; isolated Cargo staging
// deliberately omits those inputs and therefore uses a different copy routine.
fn copy_snapshot(source: &Path, destination: &Path) -> Result<()> {
    for entry in WalkDir::new(source).follow_links(false) {
        let entry = entry?;
        let target = destination.join(entry.path().strip_prefix(source)?);
        if entry.file_type().is_dir() {
            fs::create_dir_all(target)?;
        } else if entry.file_type().is_symlink() {
            let link = fs::read_link(entry.path())?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(link, target)?;
            #[cfg(windows)]
            if entry.path().is_dir() {
                std::os::windows::fs::symlink_dir(link, target)?;
            } else {
                std::os::windows::fs::symlink_file(link, target)?;
            }
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
fn capture(
    root: &Path,
    source: &Path,
    revision: &str,
    version: &str,
    expected: Option<&str>,
) -> Result<Json> {
    ensure!(stable_version(version), "Release must look like 1.27.0");
    let sha = git(
        source,
        &["rev-parse", "--verify", &format!("{revision}^{{commit}}")],
    )?;
    if let Some(expected) = expected {
        ensure!(
            full_sha(expected) && expected.eq_ignore_ascii_case(&sha),
            "Expected {expected}, found {sha}"
        );
    }
    let temp = tempfile::tempdir()?;
    let archive = temp.path().join("release.tar");
    checked(
        Command::new("git")
            .args(["archive", "--format=tar"])
            .arg(format!("--output={}", archive.display()))
            .arg(&sha)
            .current_dir(source),
    )?;
    let snapshot = temp.path().join("snapshot");
    fs::create_dir(&snapshot)?;
    let mut bundle = tar::Archive::new(fs::File::open(&archive)?);
    for entry in bundle.entries()? {
        let mut entry = entry?;
        // git archive records its source commit in a global PAX header.
        if entry.header().entry_type().is_pax_global_extensions() {
            continue;
        }
        safe_relative(&entry.path()?)?;
        if entry.header().entry_type().is_symlink() {
            let link = entry.link_name()?.context("Missing symlink target")?;
            safe_link(&entry.path()?, &link)?;
        }
        ensure!(
            entry.header().entry_type().is_file()
                || entry.header().entry_type().is_dir()
                || entry.header().entry_type().is_symlink(),
            "Unsupported upstream archive entry"
        );
        ensure!(entry.unpack_in(&snapshot)?, "Unsafe archive path");
    }
    let mut packages = BTreeMap::new();
    for entry in fs::read_dir(snapshot.join("crates"))? {
        let path = entry?.path().join("Cargo.toml");
        if !path.is_file() {
            continue;
        }
        let doc = read_toml(&path)?;
        if doc.get("package").is_none() {
            continue;
        }
        let name = str_at(&doc["package"], "name");
        ensure!(
            doc["package"]["version"] == version,
            "{name} has unexpected version"
        );
        ensure!(
            packages
                .insert(
                    name.to_owned(),
                    format!(
                        "gpui/{}",
                        path.parent().unwrap().strip_prefix(&snapshot)?.display()
                    )
                )
                .is_none(),
            "Duplicate package"
        );
    }
    ensure!(
        packages.contains_key("gpui-unofficial"),
        "Missing crates/gpui-unofficial/Cargo.toml"
    );
    let staging = snapshot.join("crates/transformed-crates");
    if staging.is_symlink() {
        fs::remove_file(staging)?;
    } else if staging.exists() {
        fs::remove_dir_all(staging)?;
    }
    let mut provenance = json!({"repository":SOURCE.trim_end_matches(".git"),"ref":format!("release/{version}"),"commit":sha,"destination":"gpui","packages":packages});
    if snapshot.join("crates/transform-metadata.json").exists() {
        let metadata = read_json(snapshot.join("crates/transform-metadata.json"))?;
        provenance["zed_commit"] = metadata["zed_commit"].clone();
        provenance["zed_tag"] = metadata["zed_tag"].clone();
    }
    let destination = root.join("gpui");
    if destination.exists() {
        fs::remove_dir_all(&destination)?;
    }
    fs::create_dir_all(root)?;
    if fs::rename(&snapshot, &destination).is_err() {
        copy_snapshot(&snapshot, &destination)?;
    }
    let mut config =
        String::from("# Generated by xgpui-release import; do not edit.\n[patch.crates-io]\n");
    for (name, path) in packages {
        config += &format!("{name:?} = {{ path = {path:?} }}\n");
    }
    fs::create_dir_all(root.join(".cargo"))?;
    fs::write(root.join(".cargo/config.toml"), config)?;
    write_json(root.join("upstream.json"), &provenance)?;
    Ok(provenance)
}
fn signature(path: &Path) -> Result<Json> {
    if path.is_symlink() {
        return Ok(json!(format!("symlink:{}", fs::read_link(path)?.display())));
    }
    if !path.is_file() {
        return Ok(Json::Null);
    }
    let executable = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::metadata(path)?.permissions().mode() & 0o111 != 0
        }
        #[cfg(not(unix))]
        {
            false
        }
    };
    Ok(json!(format!(
        "{}:{}",
        if executable { "executable" } else { "file" },
        sha(&fs::read(path)?)
    )))
}
fn imported(root: &Path) -> Result<BTreeSet<String>> {
    let mut files = BTreeSet::new();
    for scope in SCOPES {
        let path = root.join(scope);
        if path.is_dir() && !path.is_symlink() {
            for e in WalkDir::new(&path).follow_links(false) {
                let e = e?;
                if e.file_type().is_file() || e.file_type().is_symlink() {
                    files.insert(
                        e.path()
                            .strip_prefix(root)?
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        } else if path.exists() || path.is_symlink() {
            files.insert(scope.into());
        }
    }
    files.retain(|p| !p.starts_with(STAGING));
    Ok(files)
}
fn classify(path: &str, packages: &BTreeSet<String>) -> &'static str {
    if path.ends_with("/Cargo.toml") || path == ".cargo/config.toml" {
        "cargo"
    } else if packages.iter().any(|p| {
        path.strip_prefix(&format!("{p}/"))
            .is_some_and(|r| r != "README.md" && r != "Cargo.toml")
    }) {
        "code"
    } else {
        "metadata"
    }
}
fn compare(root: &Path, before: &Path, incoming: &Path, version: &str) -> Result<Json> {
    let previous = read_json(before.join("upstream.json"))?;
    let source = read_json(incoming.join("upstream.json"))?;
    let packages: BTreeSet<_> = [&previous, &source]
        .into_iter()
        .flat_map(|d| {
            d["packages"]
                .as_object()
                .unwrap()
                .values()
                .filter_map(Json::as_str)
                .map(str::to_owned)
        })
        .collect();
    let mut tracked: BTreeSet<_> = git(root, &["ls-files", "--", SCOPES[0], SCOPES[1], SCOPES[2]])?
        .lines()
        .map(str::to_owned)
        .collect();
    tracked.extend(imported(incoming)?);
    let mut entries = Vec::new();
    for name in tracked {
        let local = signature(&root.join(&name))?;
        let old = signature(&before.join(&name))?;
        let new = if name.starts_with(STAGING) {
            Json::Null
        } else {
            signature(&incoming.join(&name))?
        };
        if local == new {
            continue;
        }
        entries.push(json!({"path":name,"kind":classify(&name,&packages),"retain_local":!name.starts_with(STAGING)&&old==new&&old!=local,"change":if local.is_null(){"added"}else if new.is_null(){"deleted"}else{"modified"},"upstream_changed":old!=new,"downstream_overlap":old!=local,"local":local,"incoming":new}));
    }
    Ok(
        json!({"release":version,"base_commit":git(root,&["rev-parse","HEAD"])?,"source_commit":source["commit"],"previous_source_commit":previous["commit"],"entries":entries}),
    )
}
fn apply(root: &Path, incoming: &Path, report: &Json, kind: &str) -> Result<()> {
    ensure!(
        matches!(kind, "code" | "cargo" | "metadata"),
        "Unknown review kind"
    );
    // Downstream changes win: a file XGPUI changed relative to the previous
    // upstream baseline is never replaced or deleted wholesale. Those
    // overlaps are ported by hand, keeping local behavior. Entries that do not
    // record `downstream_overlap` are treated as overlaps.
    let entries: Vec<_> = rows(&report["entries"])
        .iter()
        .filter(|e| {
            e["kind"] == kind && e["retain_local"] != true && e["downstream_overlap"] == false
        })
        .collect();
    for entry in &entries {
        let name = str_at(entry, "path");
        let relative = Path::new(name);
        safe_relative(relative)?;
        ensure!(
            name.starts_with("gpui/") || SCOPES.contains(&name),
            "Outside import scope"
        );
        for parent in relative.ancestors().skip(1) {
            ensure!(!root.join(parent).is_symlink(), "Symlink parent: {name}");
        }
        let expected = if name.starts_with(STAGING) {
            Json::Null
        } else {
            signature(&incoming.join(name))?
        };
        ensure!(
            signature(&root.join(name))? == entry["local"] && expected == entry["incoming"],
            "Tree changed since review plan: {name}"
        );
    }
    for entry in entries {
        let name = str_at(entry, "path");
        let dest = root.join(name);
        if dest.is_file() || dest.is_symlink() {
            fs::remove_file(&dest)?;
        }
        if !entry["incoming"].is_null() {
            fs::create_dir_all(dest.parent().unwrap())?;
            let source = incoming.join(name);
            if source.is_symlink() {
                let target = fs::read_link(&source)?;
                #[cfg(unix)]
                std::os::unix::fs::symlink(target, &dest)?;
                #[cfg(windows)]
                std::os::windows::fs::symlink_file(target, &dest)?;
            } else {
                fs::copy(source, dest)?;
            }
        }
    }
    Ok(())
}
fn context_inputs(before: &Path, incoming: &Path) -> Result<Json> {
    let mut captures = Vec::new();
    let mut paths = BTreeSet::new();
    for root in [before, incoming] {
        let p = read_json(root.join("upstream.json"))?;
        let metadata = read_json(root.join("gpui/crates/transform-metadata.json"))?;
        let sha = str_at(&p, "zed_commit");
        ensure!(
            full_sha(sha) && metadata["zed_commit"] == sha,
            "Missing or inconsistent exact Zed SHA"
        );
        let source = p.get("source_packages").unwrap_or(&p["packages"]);
        let crates = rows(&metadata["crates"]);
        let names = strings(&metadata["crates"]);
        ensure!(
            !crates.is_empty()
                && names.len() == crates.len()
                && names.len()
                    == source
                        .as_object()
                        .context("Missing captured inventory")?
                        .len(),
            "Zed crate inventory does not cover captured packages"
        );
        for name in names {
            safe_relative(Path::new(&name))?;
            paths.insert(format!("crates/{name}"));
        }
        captures.push(sha.to_owned());
    }
    Ok(
        json!({"repository":"zed-industries/zed","base_commit":captures[0],"head_commit":captures[1],"paths":paths}),
    )
}
fn api(endpoint: &str) -> Result<Json> {
    Ok(serde_json::from_str(&output(
        &std::env::current_dir()?,
        "gh",
        &["api", "--paginate", "--slurp", endpoint],
    )?)?)
}
fn collect(inputs: &Json) -> Result<Json> {
    collect_with(inputs, api)
}
fn collect_with(inputs: &Json, mut api: impl FnMut(&str) -> Result<Json>) -> Result<Json> {
    let base = str_at(inputs, "base_commit");
    let head = str_at(inputs, "head_commit");
    ensure!(
        inputs["repository"] == "zed-industries/zed" && full_sha(base) && full_sha(head),
        "Context requires pinned Zed commit SHAs"
    );
    let pages = api(&format!(
        "repos/zed-industries/zed/compare/{base}...{head}?per_page=100"
    ))?;
    ensure!(
        matches!(str_at(&pages[0], "status"), "ahead" | "identical"),
        "Zed commits do not form forward range"
    );
    let mut commits = Vec::new();
    let mut seen = BTreeSet::new();
    for page in rows(&pages) {
        for commit in rows(&page["commits"]) {
            let sha = str_at(commit, "sha").to_owned();
            if seen.insert(sha.clone()) {
                commits.push((sha, commit.clone()));
            }
        }
    }
    ensure!(
        commits.len() as u64 == pages[0]["total_commits"].as_u64().unwrap_or(u64::MAX),
        "Incomplete Zed commit pagination"
    );
    let paths = strings(&inputs["paths"]);
    let mut entries = Vec::new();
    for (sha, commit) in commits {
        let pages = api(&format!(
            "repos/zed-industries/zed/commits/{sha}?per_page=100"
        ))?;
        let files: Vec<_> = rows(&pages)
            .iter()
            .flat_map(|p| rows(&p["files"]))
            .collect();
        ensure!(
            files.len() < 3000,
            "Zed commit may exceed API file limit; inspect with Git"
        );
        let names: BTreeSet<_> = files
            .iter()
            .flat_map(|e| {
                [e["filename"].as_str(), e["previous_filename"].as_str()]
                    .into_iter()
                    .flatten()
            })
            .collect();
        let captured: Vec<_> = names
            .iter()
            .filter(|n| {
                paths
                    .iter()
                    .any(|p| **n == p || n.starts_with(&format!("{p}/")))
            })
            .collect();
        let shared: Vec<_> = names
            .iter()
            .filter(|n| matches!(**n, "Cargo.toml" | "Cargo.lock"))
            .collect();
        entries.push(json!({"sha":sha,"subject":str_at(&commit["commit"],"message").lines().next().unwrap_or(""),"captured_paths":captured,"workspace_paths":shared}));
    }
    let mut report = inputs.clone();
    report["commits"] = json!(entries);
    Ok(report)
}
fn link_mentions(text: &str) -> String {
    let mut chars = text.chars().peekable();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        if c == '#' {
            let mut digits = String::new();
            while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                digits.push(chars.next().unwrap());
            }
            if digits.is_empty() {
                out.push('#');
            } else {
                out += &format!("[#{digits}](https://github.com/zed-industries/zed/pull/{digits})");
            }
        } else if c == '@' {
            out += "&#64;";
        } else {
            out.push(c);
        }
    }
    out
}
fn markdown(context: &Json) -> String {
    let base = str_at(context, "base_commit");
    let head = str_at(context, "head_commit");
    let mut out = format!(
        "## Zed release context\n\nExact provenance range: [`{base}` → `{head}`](https://github.com/zed-industries/zed/compare/{base}...{head}).\n\nScope: **{} captured crate paths**, union of both release inventories. Shared root Cargo files are reported separately.\n\nThis is upstream context, not a claim that every Zed change is present in captured crates.\n",
        rows(&context["paths"]).len()
    );
    for (title, kind) in [
        ("Captured-crate commits", 0),
        ("Shared workspace dependency context", 1),
        ("Other commits in the exact range", 2),
    ] {
        out += &format!("\n### {title}\n\n");
        let selected: Vec<_> = rows(&context["commits"])
            .iter()
            .filter(|c| match kind {
                0 => !rows(&c["captured_paths"]).is_empty(),
                1 => !rows(&c["workspace_paths"]).is_empty(),
                _ => {
                    rows(&c["captured_paths"]).is_empty() && rows(&c["workspace_paths"]).is_empty()
                }
            })
            .collect();
        if selected.is_empty() {
            out += "None.\n";
        }
        for c in selected {
            let sha = str_at(c, "sha");
            let subject = link_mentions(str_at(c, "subject"));
            out += &format!(
                "- [`{}`](https://github.com/zed-industries/zed/commit/{sha}) — {subject}\n",
                &sha[..sha.len().min(10)]
            );
            let relevant = if kind == 0 {
                &c["captured_paths"]
            } else {
                &c["workspace_paths"]
            };
            if !rows(relevant).is_empty() {
                out += &format!(
                    "  - Relevant paths: {}\n",
                    strings(relevant)
                        .into_iter()
                        .map(|p| format!("`{p}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
    }
    out += "\n<details><summary>All captured Zed crate paths</summary>\n\n";
    for path in strings(&context["paths"]) {
        out += &format!("- `{path}`\n");
    }
    out += "\n</details>\n";
    out
}
fn review_notes(report: &Json, context: &str) -> String {
    let proposed: Vec<_> = rows(&report["entries"])
        .iter()
        .filter(|e| e["retain_local"] != true)
        .collect();
    let retained: Vec<_> = rows(&report["entries"])
        .iter()
        .filter(|e| e["retain_local"] == true)
        .collect();
    let mut out = format!(
        "Incoming gpui-unofficial {} tree proposal.\nSource commit: `{}`.\n\nUnintegrated proposal: complete incoming files, not resolved patches. Downstream changes win: files marked NOT APPLIED keep local contents and need a manual port that preserves local behavior.\n",
        str_at(report, "release"),
        str_at(report, "source_commit")
    );
    for kind in ["code", "cargo", "metadata"] {
        out += &format!("\n## {kind} differences\n\n");
        for e in proposed.iter().filter(|e| e["kind"] == kind) {
            out += &format!(
                "- `{}` — {}; {}{}\n",
                str_at(e, "path"),
                str_at(e, "change"),
                if e["upstream_changed"] == true {
                    "upstream changed"
                } else {
                    "upstream unchanged"
                },
                if e["downstream_overlap"] != false {
                    "; NOT APPLIED: downstream differs, port manually and keep local behavior"
                } else {
                    ""
                }
            );
        }
        let var = format!("{}_COMMIT", kind.to_uppercase());
        if let Ok(commit) = std::env::var(&var)
            && !commit.is_empty()
        {
            out += &format!("\n{kind} commit: `{commit}`\n");
        }
    }
    out += "\n## Retained downstream changes\n\n";
    for e in retained {
        out += &format!("- `{}` ({})\n", str_at(e, "path"), str_at(e, "kind"));
    }
    out += &format!("\n{context}");
    out
}
pub(super) fn cli(root: &Path, args: &[String]) -> Result<()> {
    let o = Options(args);
    match args[0].as_str() {
        "context" => {
            let inputs = read_json(o.required("--report")?)?["zed_context"].clone();
            let context = collect(&inputs)?;
            fs::write(o.required("--output")?, markdown(&context))?;
            write_json(o.required("--json-output")?, &context)
        }
        "review-notes" => {
            let report = read_json(o.required("--report")?)?;
            let context = fs::read_to_string(o.required("--context")?)?;
            fs::write(o.required("--output")?, review_notes(&report, &context))?;
            Ok(())
        }
        "import" => {
            let version = args.get(1).context("Missing release version")?;
            ensure!(
                git(root, &["branch", "--show-current"])? == format!("release/{version}"),
                "Imports must run on release/{version} branch"
            );
            ensure!(
                git(root, &["status", "--porcelain"])?.is_empty(),
                "Checkout must be clean"
            );
            let temp = tempfile::tempdir()?;
            let (source, revision) = if let Some(source) = o.get("--source") {
                (PathBuf::from(source), "HEAD")
            } else {
                git(temp.path(), &["init", "-q"])?;
                git(
                    temp.path(),
                    &[
                        "fetch",
                        "--depth=1",
                        SOURCE,
                        &format!("refs/heads/release/{version}"),
                    ],
                )?;
                (temp.path().to_owned(), "FETCH_HEAD")
            };
            print_json(&capture(
                root,
                &source,
                revision,
                version,
                o.get("--expected-sha"),
            )?)
        }
        "review" => {
            ensure!(
                git(root, &["status", "--porcelain"])?.is_empty(),
                "Review preparation/application requires a clean checkout"
            );
            let action = args.get(1).context("Missing review action")?;
            let snapshot = absolute(Path::new(o.required("--snapshot")?))?;
            let report_path = Path::new(o.required("--report")?);
            if action != "prepare" {
                let branch = git(root, &["branch", "--show-current"])?;
                ensure!(
                    review_branch(&branch),
                    "Apply only on a dedicated review branch (sync/*, codex/* or issue-N/task), not {branch:?}"
                );
                return apply(root, &snapshot, &read_json(report_path)?, action);
            }
            let version = o.required("--release")?;
            ensure!(stable_version(version), "Invalid release version");
            ensure!(!snapshot.exists(), "Snapshot destination must not exist");
            let normalizer = absolute(Path::new(o.required("--normalizer")?))?;
            let map = absolute(Path::new(o.required("--map")?))?;
            let policies = [
                (
                    "scripts/registry-crates.json",
                    fs::read(root.join("scripts/registry-crates.json"))?,
                ),
                (
                    "scripts/release-packages.json",
                    fs::read(root.join("scripts/release-packages.json"))?,
                ),
            ];
            let previous = read_json(root.join("upstream.json"))?;
            let temp = tempfile::tempdir()?;
            let source = temp.path().join("source");
            fs::create_dir(&source)?;
            git(&source, &["init", "-q"])?;
            git(
                &source,
                &["fetch", "--depth=1", SOURCE, str_at(&previous, "commit")],
            )?;
            git(
                &source,
                &[
                    "fetch",
                    "--depth=1",
                    SOURCE,
                    &format!("refs/heads/release/{version}"),
                ],
            )?;
            let before = temp.path().join("before");
            capture(
                &before,
                &source,
                str_at(&previous, "commit"),
                str_at(&previous, "ref").trim_start_matches("release/"),
                Some(str_at(&previous, "commit")),
            )?;
            capture(
                &snapshot,
                &source,
                "FETCH_HEAD",
                version,
                o.get("--expected-sha"),
            )?;
            for tree in [&before, &snapshot] {
                for (name, contents) in &policies {
                    fs::create_dir_all(tree.join(name).parent().unwrap())?;
                    fs::write(tree.join(name), contents)?;
                }
                for check in [false, true] {
                    let mut cmd = Command::new(&normalizer);
                    cmd.arg("normalize")
                        .arg("--root")
                        .arg(tree)
                        .arg("--map")
                        .arg(&map);
                    if check {
                        cmd.arg("--check");
                    }
                    checked(&mut cmd)?;
                }
            }
            let mut report = compare(root, &before, &snapshot, version)?;
            report["zed_context"] = context_inputs(&before, &snapshot)?;
            write_json(report_path, &report)
        }
        _ => bail!("Unknown upstream command"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_and_unsafe_review_plans_write_nothing() {
        let root = tempfile::tempdir().unwrap();
        let incoming = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("gpui")).unwrap();
        fs::write(root.path().join("gpui/a"), "local").unwrap();
        let report = json!({"entries":[{"path":"gpui/a","kind":"code","downstream_overlap":false,"local":"stale","incoming":null}]});
        assert!(apply(root.path(), incoming.path(), &report, "code").is_err());
        assert_eq!(
            fs::read_to_string(root.path().join("gpui/a")).unwrap(),
            "local"
        );
        let report = json!({"entries":[{"path":"../outside","kind":"code","downstream_overlap":false,"local":null,"incoming":null}]});
        assert!(apply(root.path(), incoming.path(), &report, "code").is_err());
    }
    #[test]
    fn review_application_branches() {
        for ok in [
            "sync/1.28.1",
            "codex/review",
            "issue-284/sync-tooling",
            "issue-284/core-cache",
            "issue-7/a/b",
        ] {
            assert!(review_branch(ok), "{ok}");
        }
        for bad in [
            "",
            "main",
            "release/1.28.1",
            "issue-284/epic",
            "issue-284/",
            "issue-/task",
            "issue-28x/task",
            "issue-284",
            "issue-284/../main",
            "issue-284/a//b",
            "sync/",
            "codex/",
            "feature/sync/x",
        ] {
            assert!(!review_branch(bad), "{bad}");
        }
    }
    #[test]
    fn downstream_overlaps_are_never_overwritten_or_deleted() {
        let root = tempfile::tempdir().unwrap();
        let incoming = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("gpui")).unwrap();
        fs::create_dir_all(incoming.path().join("gpui")).unwrap();
        for name in ["edited", "deleted_upstream", "legacy", "clean"] {
            fs::write(root.path().join("gpui").join(name), "ours").unwrap();
        }
        for name in ["edited", "legacy", "clean"] {
            fs::write(incoming.path().join("gpui").join(name), "upstream").unwrap();
        }
        let ours = signature(&root.path().join("gpui/clean")).unwrap();
        let upstream = signature(&incoming.path().join("gpui/clean")).unwrap();
        let report = json!({"entries":[
            {"path":"gpui/edited","kind":"code","retain_local":false,"downstream_overlap":true,"local":ours,"incoming":upstream},
            {"path":"gpui/deleted_upstream","kind":"code","retain_local":false,"downstream_overlap":true,"local":ours,"incoming":null},
            {"path":"gpui/legacy","kind":"code","retain_local":false,"local":ours,"incoming":upstream},
            {"path":"gpui/clean","kind":"code","retain_local":false,"downstream_overlap":false,"local":ours,"incoming":upstream}
        ]});
        apply(root.path(), incoming.path(), &report, "code").unwrap();
        for name in ["edited", "deleted_upstream", "legacy"] {
            assert_eq!(
                fs::read_to_string(root.path().join("gpui").join(name)).unwrap(),
                "ours",
                "{name}"
            );
        }
        assert_eq!(
            fs::read_to_string(root.path().join("gpui/clean")).unwrap(),
            "upstream"
        );
    }
    #[test]
    fn retain_local_entries_are_not_applied() {
        let root = tempfile::tempdir().unwrap();
        let incoming = tempfile::tempdir().unwrap();
        let report = json!({"entries":[{"path":"gpui/a","kind":"code","retain_local":true,"local":"ignored","incoming":null}]});
        apply(root.path(), incoming.path(), &report, "code").unwrap();
    }
    #[test]
    fn context_requires_exact_inventory_and_sha() {
        let before = tempfile::tempdir().unwrap();
        let incoming = tempfile::tempdir().unwrap();
        for root in [before.path(), incoming.path()] {
            fs::create_dir_all(root.join("gpui/crates")).unwrap();
            write_json(
                root.join("upstream.json"),
                &json!({"zed_commit":"a".repeat(40),"packages":{"gpui":"gpui/crates/gpui"}}),
            )
            .unwrap();
            write_json(
                root.join("gpui/crates/transform-metadata.json"),
                &json!({"zed_commit":"a".repeat(40),"crates":["gpui"]}),
            )
            .unwrap();
        }
        assert_eq!(
            context_inputs(before.path(), incoming.path()).unwrap()["paths"],
            json!(["crates/gpui"])
        );
        write_json(
            incoming.path().join("gpui/crates/transform-metadata.json"),
            &json!({"zed_commit":"wrong","crates":["gpui"]}),
        )
        .unwrap();
        assert!(context_inputs(before.path(), incoming.path()).is_err());
    }
}
#[cfg(test)]
mod import_regressions {
    use super::*;
    fn source() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        git(r, &["init", "-q"]).unwrap();
        git(r, &["config", "user.name", "Fixture"]).unwrap();
        git(r, &["config", "user.email", "fixture@example.test"]).unwrap();
        fs::create_dir_all(r.join("crates/gpui-unofficial")).unwrap();
        fs::write(
            r.join("crates/gpui-unofficial/Cargo.toml"),
            "[package]\nname='gpui-unofficial'\nversion='1.27.0'\n",
        )
        .unwrap();
        fs::write(r.join("crates/gpui-unofficial/old.rs"), "old").unwrap();
        commit(r);
        t
    }
    fn commit(root: &Path) {
        git(root, &["add", "--all"]).unwrap();
        git(root, &["commit", "-qm", "fixture"]).unwrap();
    }
    #[test]
    fn immutable_capture_replacement_and_provenance() {
        let source = source();
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("docs")).unwrap();
        fs::write(root.path().join("docs/local.md"), "local").unwrap();
        let a = capture(root.path(), source.path(), "HEAD", "1.27.0", None).unwrap();
        let b = capture(root.path(), source.path(), "HEAD", "1.27.0", None).unwrap();
        assert_eq!(a, b);
        assert_eq!(
            a["commit"],
            git(source.path(), &["rev-parse", "HEAD"]).unwrap()
        );
        fs::remove_file(source.path().join("crates/gpui-unofficial/old.rs")).unwrap();
        fs::write(source.path().join("crates/gpui-unofficial/new.rs"), "new").unwrap();
        commit(source.path());
        capture(root.path(), source.path(), "HEAD", "1.27.0", None).unwrap();
        assert!(
            !root
                .path()
                .join("gpui/crates/gpui-unofficial/old.rs")
                .exists()
        );
        assert!(
            root.path()
                .join("gpui/crates/gpui-unofficial/new.rs")
                .exists()
        );
        assert_eq!(
            fs::read_to_string(root.path().join("docs/local.md")).unwrap(),
            "local"
        );
    }
    #[test]
    fn wrong_sha_or_invalid_snapshot_does_not_replace_capture() {
        let source = source();
        let root = tempfile::tempdir().unwrap();
        let original = capture(root.path(), source.path(), "HEAD", "1.27.0", None).unwrap();
        assert!(
            capture(
                root.path(),
                source.path(),
                "HEAD",
                "1.27.0",
                Some(&"0".repeat(40))
            )
            .is_err()
        );
        fs::remove_file(source.path().join("crates/gpui-unofficial/Cargo.toml")).unwrap();
        commit(source.path());
        assert!(capture(root.path(), source.path(), "HEAD", "1.27.0", None).is_err());
        assert_eq!(
            read_json(root.path().join("upstream.json")).unwrap(),
            original
        );
    }
    #[test]
    fn staging_mirror_removed_and_vendor_sources_preserved() {
        let source = source();
        let root = tempfile::tempdir().unwrap();
        for p in [
            "crates/transformed-crates/old.rs",
            "crates/gpui-unofficial/gpui/src/scene.rs",
        ] {
            fs::create_dir_all(source.path().join(p).parent().unwrap()).unwrap();
            fs::write(source.path().join(p), "code").unwrap();
        }
        commit(source.path());
        capture(root.path(), source.path(), "HEAD", "1.27.0", None).unwrap();
        assert!(!root.path().join("gpui/crates/transformed-crates").exists());
        assert!(
            root.path()
                .join("gpui/crates/gpui-unofficial/gpui/src/scene.rs")
                .exists()
        );
    }
    #[test]
    fn classification_distinguishes_manifest_code_and_metadata() {
        let p = BTreeSet::from(["gpui/crates/gpui".into()]);
        assert_eq!(classify("gpui/crates/gpui/Cargo.toml", &p), "cargo");
        assert_eq!(classify("gpui/crates/gpui/src/lib.rs", &p), "code");
        assert_eq!(classify("gpui/crates/gpui/README.md", &p), "metadata");
        assert_eq!(classify("upstream.json", &p), "metadata");
    }
    #[cfg(unix)]
    #[test]
    fn symlink_parent_review_escape_rejected() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let incoming = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("gpui")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("gpui/escape")).unwrap();
        let report = json!({"entries":[{"path":"gpui/escape/a","kind":"code","downstream_overlap":false,"local":null,"incoming":null}]});
        assert!(apply(root.path(), incoming.path(), &report, "code").is_err());
        assert!(!outside.path().join("a").exists());
    }
}
#[cfg(test)]
mod context_regressions {
    use super::*;
    #[test]
    fn context_pagination_rename_paths_order_and_mentions() {
        let inputs = json!({"repository":"zed-industries/zed","base_commit":"a".repeat(40),"head_commit":"b".repeat(40),"paths":["crates/old","crates/new"]});
        let mut requests = 0;
        let context=collect_with(&inputs,|endpoint|{requests+=1;if endpoint.contains("/compare/"){Ok(json!([{"status":"ahead","total_commits":2,"commits":[{"sha":"c".repeat(40),"commit":{"message":"Rename #42 @person"}}]},{"commits":[{"sha":"a".repeat(40),"commit":{"message":"Shared Cargo"}}]}]))}else if endpoint.contains(&"c".repeat(40)){Ok(json!([{"files":[{"filename":"crates/new/src/lib.rs","previous_filename":"crates/old/src/lib.rs"}]}]))}else{Ok(json!([{"files":[{"filename":"Cargo.toml"}]}]))}}).unwrap();
        assert_eq!(requests, 3);
        assert_eq!(context["commits"][0]["sha"], "c".repeat(40));
        assert_eq!(
            context["commits"][0]["captured_paths"],
            json!(["crates/new/src/lib.rs", "crates/old/src/lib.rs"])
        );
        let text = markdown(&context);
        assert!(text.contains("[#42](https://github.com/zed-industries/zed/pull/42)"));
        assert!(text.contains("&#64;person"));
    }
    #[test]
    fn incomplete_or_reverse_context_fails_closed() {
        let inputs = json!({"repository":"zed-industries/zed","base_commit":"a".repeat(40),"head_commit":"b".repeat(40),"paths":[]});
        for response in [
            json!([{"status":"behind","total_commits":0,"commits":[]}]),
            json!([{"status":"ahead","total_commits":2,"commits":[]}]),
        ] {
            assert!(collect_with(&inputs, |_| Ok(response.clone())).is_err());
        }
    }
}
#[cfg(test)]
mod snapshot_copy_regressions {
    use super::*;
    #[test]
    fn snapshot_copy_preserves_configuration_and_lockfiles() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        fs::create_dir(source.path().join(".cargo")).unwrap();
        fs::write(source.path().join("Cargo.lock"), "lock").unwrap();
        fs::write(source.path().join(".cargo/config.toml"), "config").unwrap();
        copy_snapshot(source.path(), destination.path()).unwrap();
        assert_eq!(
            fs::read_to_string(destination.path().join("Cargo.lock")).unwrap(),
            "lock"
        );
        assert_eq!(
            fs::read_to_string(destination.path().join(".cargo/config.toml")).unwrap(),
            "config"
        );
    }
    #[test]
    fn link_targets_can_use_safe_parent_but_cannot_escape() {
        assert!(safe_link(Path::new("crates/a/link"), Path::new("../b/file")).is_ok());
        assert!(safe_link(Path::new("crates/a/link"), Path::new("../../../escape")).is_err());
        assert!(safe_link(Path::new("crates/a/link"), Path::new("/absolute")).is_err());
    }
}
