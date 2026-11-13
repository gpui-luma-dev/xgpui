use super::*;
use flate2::read::GzDecoder;
use std::io::Cursor;
pub(super) fn archive_contents(data: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut archive = tar::Archive::new(GzDecoder::new(Cursor::new(data)));
    let mut files = BTreeMap::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        safe_relative(&path)?;
        ensure!(
            entry.header().entry_type().is_file() || entry.header().entry_type().is_dir(),
            "Unexpected archive entry {}",
            path.display()
        );
        if entry.header().entry_type().is_file() {
            let name = path
                .to_str()
                .context("Non-UTF8 archive path")?
                .replace('\\', "/");
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            ensure!(
                files.insert(name, bytes).is_none(),
                "Duplicate archive entry"
            );
        }
    }
    Ok(files)
}
pub(super) fn payload(data: &[u8], name: &str, version: &str) -> Result<BTreeMap<String, Vec<u8>>> {
    let prefix = format!("{name}-{version}/");
    let files = archive_contents(data)?;
    ensure!(
        !files.is_empty() && files.keys().all(|k| k.starts_with(&prefix)),
        "{name}: unexpected archive root"
    );
    let mut files: BTreeMap<_, _> = files
        .into_iter()
        .map(|(k, v)| (k[prefix.len()..].to_owned(), v))
        .collect();
    let mut manifest = parse_toml(std::str::from_utf8(
        files
            .get("Cargo.toml")
            .context("Missing normalized manifest")?,
    )?)?;
    ensure!(
        manifest["package"]["name"] == name && manifest["package"]["version"] == version,
        "{name}: archive identity mismatch"
    );
    manifest["package"]
        .as_object_mut()
        .unwrap()
        .remove("publish");
    for generated in [
        "Cargo.toml",
        "Cargo.toml.orig",
        "Cargo.lock",
        ".cargo_vcs_info.json",
    ] {
        files.remove(generated);
    }
    files.insert("Cargo.toml".into(), serde_json::to_vec(&manifest)?);
    Ok(files)
}
pub(super) fn isolated_command(out: &Path, program: &str) -> Result<Command> {
    let home = out.join("cargo-home");
    fs::create_dir_all(&home)?;
    let cache = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|p| PathBuf::from(p).join(".cargo"))
        })
        .context("Cannot locate registry cache")?
        .join("registry");
    if cache.is_dir() && !home.join("registry").exists() {
        #[cfg(unix)]
        std::os::unix::fs::symlink(cache.canonicalize()?, home.join("registry"))?;
        #[cfg(windows)]
        {
            // Avoid requiring Windows symlink privileges; use a separate empty cache.
            let _ = cache;
        }
    }
    let mut command = Command::new(program);
    command.current_dir(out);
    for (key, _) in std::env::vars_os() {
        let k = key.to_string_lossy();
        if k.starts_with("CARGO_")
            || k.starts_with("RUSTUP_")
            || [
                "RUSTFLAGS",
                "RUSTDOCFLAGS",
                "RUSTC_WRAPPER",
                "RUSTC_WORKSPACE_WRAPPER",
                "RUSTC",
                "RUSTDOC",
            ]
            .contains(&k.as_ref())
        {
            command.env_remove(key);
        }
    }
    command
        .env("CARGO_HOME", home)
        .env("CARGO_TARGET_DIR", out.join("target"))
        .env("CLANG_MODULE_CACHE_PATH", out.join("clang-cache"))
        .env("CARGO_BUILD_JOBS", "4")
        .env("CARGO_HTTP_TIMEOUT", "30")
        .env("CARGO_NET_RETRY", "1")
        .env("CARGO_TERM_COLOR", "never");
    Ok(command)
}
fn execute(
    out: &Path,
    row: &mut Json,
    args: &[String],
    token: Option<&str>,
) -> Result<(i32, String)> {
    let number = rows(&row["commands"]).len() + 1;
    let log = out.join(format!("{}-{number}.log", str_at(row, "name")));
    let raw = tempfile::NamedTempFile::new_in(out)?;
    let stream = raw.reopen()?;
    let mut command = isolated_command(out, "cargo")?;
    command
        .args(args)
        .arg("--quiet")
        .stdout(stream.try_clone()?)
        .stderr(stream);
    if let Some(token) = token {
        command.env("CARGO_REGISTRY_TOKEN", token);
    }
    let mut child = command.spawn()?;
    let start = std::time::Instant::now();
    let code = loop {
        if let Some(status) = child.try_wait()? {
            break status.code().unwrap_or(1);
        }
        if start.elapsed() > Duration::from_secs(1200) {
            child.kill()?;
            child.wait()?;
            break 124;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let mut text = String::from_utf8_lossy(&fs::read(raw.path())?).into_owned();
    if let Some(token) = token
        && !token.is_empty()
    {
        text = text.replace(token, "[REDACTED]");
    }
    if code == 124 {
        text += "\nTimed out; verify registry state before retrying.\n";
    }
    fs::write(&log, compact_log(&text))?;
    if row.get("commands").is_none() {
        row["commands"] = json!([]);
    }
    let mut full = vec!["cargo".to_owned()];
    full.extend_from_slice(args);
    full.push("--quiet".to_owned());
    row["commands"]
        .as_array_mut()
        .unwrap()
        .push(json!({"command":full,"exit_code":code,"log":log}));
    if code != 0 {
        eprintln!(
            "{}: Cargo failed ({code}); {}",
            str_at(row, "name"),
            log.display()
        );
    }
    Ok((code, text))
}

fn compact_log(text: &str) -> String {
    const LIMIT: usize = 64 * 1024;
    const NOTICE: &str = "[Earlier output omitted; log capped at 64 KiB.]\n";
    if text.len() <= LIMIT {
        return text.to_owned();
    }
    let mut start = text.len() - (LIMIT - NOTICE.len());
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("{NOTICE}{}", &text[start..])
}
trait Driver {
    fn package(&mut self, row: &mut Json) -> Result<()>;
    fn existing(&mut self, name: &str, version: &str) -> Result<Option<Json>>;
    fn compare(&mut self, row: &Json, entry: &Json) -> Result<Vec<String>>;
    fn verify(&mut self, row: &mut Json) -> Result<()>;
    fn upload(&mut self, row: &mut Json) -> Result<()>;
    fn save(&mut self, report: &Json) -> Result<()>;
}
struct Registry<'a> {
    out: &'a Path,
    token: String,
}
impl Driver for Registry<'_> {
    fn package(&mut self, row: &mut Json) -> Result<()> {
        let manifest = self.out.join("staged").join(str_at(row, "manifest"));
        let args = vec![
            "package".into(),
            "--no-verify".into(),
            "--exclude-lockfile".into(),
            "--manifest-path".into(),
            manifest.to_string_lossy().into_owned(),
        ];
        let (code, _) = execute(self.out, row, &args, None)?;
        ensure!(
            code == 0,
            "{}: archive generation failed; inspect saved log",
            str_at(row, "name")
        );
        let id = format!("{}-{}", str_at(row, "name"), str_at(row, "version"));
        let archive = self.out.join("target/package").join(format!("{id}.crate"));
        let files = verify::inspect_archive(
            &archive,
            manifest.parent().unwrap(),
            &self.out.join("inspected"),
            &id,
        )?;
        row["archive"] = json!(archive);
        row["archive_sha256"] = json!(sha(&fs::read(&archive)?));
        row["files"] = json!(files.len());
        payload(
            &fs::read(&archive)?,
            str_at(row, "name"),
            str_at(row, "version"),
        )?;
        fs::copy(
            &archive,
            self.out.join("prepared").join(archive.file_name().unwrap()),
        )?;
        Ok(())
    }
    fn existing(&mut self, name: &str, version: &str) -> Result<Option<Json>> {
        let Some(doc) = fetch_json(
            &format!("https://crates.io/api/v1/crates/{name}/{version}"),
            true,
        )?
        else {
            return Ok(None);
        };
        let entry = doc["version"].clone();
        ensure!(
            entry["crate"] == name && entry["num"] == version && entry["yanked"] == false,
            "{name}@{version}: unexpected registry identity or yanked version"
        );
        Ok(Some(entry))
    }
    fn compare(&mut self, row: &Json, entry: &Json) -> Result<Vec<String>> {
        let name = str_at(row, "name");
        let version = str_at(row, "version");
        let data = fetch(
            &format!("https://static.crates.io/crates/{name}/{name}-{version}.crate"),
            false,
        )?
        .unwrap();
        ensure!(
            sha(&data) == str_at(entry, "checksum"),
            "{name}: registry checksum mismatch"
        );
        let local = payload(&fs::read(str_at(row, "archive"))?, name, version)?;
        let remote = payload(&data, name, version)?;
        let keys: BTreeSet<_> = local.keys().chain(remote.keys()).cloned().collect();
        Ok(keys
            .into_iter()
            .filter(|k| local.get(k) != remote.get(k))
            .collect())
    }
    fn verify(&mut self, row: &mut Json) -> Result<()> {
        let manifest = self.out.join("staged").join(str_at(row, "manifest"));
        let (code, _) = execute(
            self.out,
            row,
            &[
                "package".into(),
                "--registry".into(),
                "crates-io".into(),
                "--manifest-path".into(),
                manifest.to_string_lossy().into_owned(),
            ],
            None,
        )?;
        ensure!(
            code == 0,
            "{}: registry-backed package/build verification failed",
            str_at(row, "name")
        );
        let archive = Path::new(str_at(row, "archive"));
        ensure!(
            payload(
                &fs::read(archive)?,
                str_at(row, "name"),
                str_at(row, "version")
            )? == payload(
                &fs::read(self.out.join("prepared").join(archive.file_name().unwrap()))?,
                str_at(row, "name"),
                str_at(row, "version")
            )?,
            "Verified package differs from prepared payload"
        );
        Ok(())
    }
    fn upload(&mut self, row: &mut Json) -> Result<()> {
        let manifest = self.out.join("staged").join(str_at(row, "manifest"));
        for attempt in 0..3 {
            // Every retry first checks visibility; never blindly repeat an upload.
            if let Some(entry) = self.existing(str_at(row, "name"), str_at(row, "version"))? {
                ensure!(
                    self.compare(row, &entry)?.is_empty(),
                    "Observed version differs"
                );
                row["registry_checksum"] = entry["checksum"].clone();
                return Ok(());
            }
            let (code, text) = execute(
                self.out,
                row,
                &[
                    "publish".into(),
                    "--locked".into(),
                    "--registry".into(),
                    "crates-io".into(),
                    "--manifest-path".into(),
                    manifest.to_string_lossy().into_owned(),
                ],
                Some(&self.token),
            )?;
            for poll in 0..13 {
                if let Some(entry) = self.existing(str_at(row, "name"), str_at(row, "version"))? {
                    ensure!(
                        self.compare(row, &entry)?.is_empty(),
                        "Uploaded/observed version differs"
                    );
                    row["registry_checksum"] = entry["checksum"].clone();
                    return Ok(());
                }
                if code != 0 || poll == 12 {
                    break;
                }
                std::thread::sleep(Duration::from_secs(5));
            }
            let deadline = retry_deadline(&text)
                .filter(|_| attempt < 2)
                .context("Upload unverified; inspect registry state before retrying")?;
            while let Ok(remaining) = deadline.duration_since(SystemTime::now()) {
                println!(
                    "{}: quota retry in {}s",
                    str_at(row, "name"),
                    remaining.as_secs()
                );
                std::thread::sleep(remaining.min(Duration::from_secs(30)));
            }
        }
        bail!("Registry quota retry limit reached")
    }
    fn save(&mut self, report: &Json) -> Result<()> {
        let temp = self.out.join("report.tmp");
        write_json(&temp, report)?;
        fs::rename(temp, self.out.join("report.json"))?;
        Ok(())
    }
}
fn retry_deadline(output: &str) -> Option<SystemTime> {
    if !output
        .split(|c: char| !c.is_ascii_digit())
        .any(|word| word == "429")
    {
        return None;
    }
    let date = output.split_once("try again after ")?.1;
    let end = date.find(" GMT")? + 4;
    httpdate::parse_http_date(&date[..end]).ok()
}

fn publication_guard(mode: &str, confirmation: &str, commit: &str, token: &str) -> Result<()> {
    ensure!(
        matches!(mode, "plan" | "dry-run" | "publish"),
        "Unknown release mode"
    );
    if mode == "publish" {
        ensure!(
            confirmation == format!("publish {commit}") && !token.is_empty(),
            "Publication requires exact confirmation: publish FULL_COMMIT_SHA, and CARGO_REGISTRY_TOKEN"
        );
    }
    Ok(())
}
fn transition(report: &mut Json, driver: &mut impl Driver) -> Result<()> {
    let mode = str_at(report, "mode").to_owned();
    let count = rows(&report["packages"]).len();
    for i in 0..count {
        let row = &mut report["packages"][i];
        driver.package(row)?;
        if let Some(entry) = driver.existing(str_at(row, "name"), str_at(row, "version"))? {
            let changes = driver.compare(row, &entry)?;
            row["status"] = json!(if changes.is_empty() {
                "already-published"
            } else {
                "version-conflict"
            });
            row["changed_files"] = json!(changes);
            row["registry_checksum"] = entry["checksum"].clone();
        } else {
            row["status"] = json!("publish-needed");
        }
        driver.save(report)?;
    }
    ensure!(
        !rows(&report["packages"])
            .iter()
            .any(|r| r["status"] == "version-conflict"),
        "Bump versions with changed published content before any upload"
    );
    for r in rows(&report["packages"]) {
        ensure!(
            r["status"] == "already-published" || allowed(&r["publish_policy"]),
            "{}: crates.io publication disabled in manifest",
            str_at(r, "name")
        );
    }
    if mode == "plan" {
        report["status"] = json!("planned");
        return Ok(());
    }
    for i in 0..count {
        if report["packages"][i]["status"] == "already-published" {
            continue;
        }
        let pending: Vec<_> = rows(&report["packages"][i]["dependencies"])
            .iter()
            .filter(|dep| {
                !rows(&report["packages"]).iter().any(|r| {
                    r["name"] == **dep
                        && matches!(
                            str_at(r, "status"),
                            "already-published" | "published-verified"
                        )
                })
            })
            .cloned()
            .collect();
        if mode == "dry-run" && !pending.is_empty() {
            report["packages"][i]["status"] = json!("blocked-unpublished-dependencies");
            report["packages"][i]["blocked_by"] = json!(pending);
            driver.save(report)?;
            continue;
        }
        driver.verify(&mut report["packages"][i])?;
        report["packages"][i]["status"] = json!("verified-not-uploaded");
        driver.save(report)?;
        if mode == "publish" {
            report["packages"][i]["status"] = json!("uploading-unverified");
            driver.save(report)?;
            let row = &mut report["packages"][i];
            if let Some(entry) = driver.existing(str_at(row, "name"), str_at(row, "version"))? {
                ensure!(
                    driver.compare(row, &entry)?.is_empty(),
                    "Observed version differs from prepared payload"
                );
                row["registry_checksum"] = entry["checksum"].clone();
            } else {
                driver.upload(row)?;
            }
            row["status"] = json!("published-verified");
            driver.save(report)?;
        }
    }
    report["status"] = json!(if mode == "publish" {
        "published-verified"
    } else {
        "dry-run-complete"
    });
    report["release_ready"] = json!(
        mode == "publish"
            && rows(&report["packages"]).iter().all(|r| matches!(
                str_at(r, "status"),
                "already-published" | "published-verified"
            ))
    );
    Ok(())
}
pub(super) fn cli(root: &Path, o: &Options) -> Result<()> {
    let mode = o.get("--mode").unwrap_or("plan");
    let default_out = root
        .parent()
        .context("Repository has no parent directory")?
        .join("xgpui-release-artifacts")
        .join(format!(
            "{mode}-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
    let out = outside(root, o.get("--out").map(Path::new).unwrap_or(&default_out))?;
    let commit = match o.get("--tag") {
        Some(tag) => git(
            root,
            &[
                "rev-parse",
                "--verify",
                &format!("refs/tags/{tag}^{{commit}}"),
            ],
        )?,
        None => git(root, &["rev-parse", "HEAD"])?,
    };
    let token = std::env::var("CARGO_REGISTRY_TOKEN").unwrap_or_default();
    publication_guard(mode, o.get("--confirmation").unwrap_or(""), &commit, &token)?;
    if o.get("--tag").is_none() {
        ensure!(
            git(root, &["status", "--porcelain"])?.is_empty(),
            "Release source must be a clean commit"
        );
    }
    ensure!(!out.exists(), "Output already exists");
    fs::create_dir_all(&out)?;
    let source = if o.get("--tag").is_some() {
        let snapshot = out.join("source");
        fs::create_dir(&snapshot)?;
        let archive = out.join("source.tar");
        git(
            root,
            &[
                "archive",
                "--format=tar",
                "--output",
                archive.to_str().context("Non-UTF8 output path")?,
                &commit,
            ],
        )?;
        tar::Archive::new(fs::File::open(&archive)?).unpack(&snapshot)?;
        fs::remove_file(archive)?;
        snapshot
    } else {
        root.to_owned()
    };
    let plan = inventory::plan(&source)?;
    let mut report = json!({"schema_version":1,"source_commit":commit,"source_tag":o.get("--tag"),"mode":mode,"uploads_enabled":mode=="publish","release_ready":false,"started_at":now(),"packages":plan["packages"]});
    let mut driver = Registry {
        out: &out,
        token: token.clone(),
    };
    driver.save(&report)?;
    let result = (|| {
        for dir in ["gpui/crates", "extensions/crates"] {
            copy_tree(&source.join(dir), &out.join("staged").join(dir))?;
        }
        fs::create_dir(out.join("prepared"))?;
        transition(&mut report, &mut driver)
    })();
    if let Err(e) = &result {
        report["status"] = json!("failed");
        let message = format!("{e:#}");
        report["error"] = json!(if token.is_empty() {
            message
        } else {
            message.replace(&token, "[REDACTED]")
        });
    }
    report["finished_at"] = json!(now());
    driver.save(&report)?;
    if !o.has("--compact") {
        print!("{}", publication_summary(&report));
        println!("Report: {}", out.join("report.json").display());
    }
    result
}

fn require_stage(report: &Json, commit: &str, stage: &str) -> Result<()> {
    let expected = if stage == "preflight" {
        "planned"
    } else {
        "dry-run-complete"
    };
    ensure!(
        report["source_commit"] == commit && report["status"] == expected,
        "Run release-{stage} successfully for this exact tag first"
    );
    Ok(())
}

pub(super) fn flow(root: &Path, args: &[String]) -> Result<()> {
    ensure!(
        args.len() == 2 || (args.len() == 4 && args[2] == "--tag"),
        "Use release-flow preflight|check|publish [--tag TAG]"
    );
    let options = Options(args);
    let stage = args.get(1).map(String::as_str).unwrap_or("");
    let mode = match stage {
        "preflight" => "plan",
        "check" => "dry-run",
        "publish" => "publish",
        _ => bail!("Use release-flow preflight|check|publish [--tag TAG]"),
    };
    let tag = match options.get("--tag") {
        Some(tag) => tag.to_owned(),
        None => git(
            root,
            &[
                "describe",
                "--tags",
                "--match",
                "release-*",
                "--abbrev=0",
                "HEAD",
            ],
        )?,
    };
    let commit = git(
        root,
        &[
            "rev-parse",
            "--verify",
            &format!("refs/tags/{tag}^{{commit}}"),
        ],
    )?;
    let state = root
        .parent()
        .context("Repository has no parent directory")?
        .join("xgpui-release-artifacts")
        .join(&commit);
    let previous = match stage {
        "check" => Some("preflight"),
        "publish" => Some("check"),
        _ => None,
    };
    if let Some(previous) = previous {
        let report = read_json(state.join(format!("{previous}.json")))
            .with_context(|| format!("Run just release-{previous} for {tag} first"))?;
        require_stage(&report, &commit, previous)?;
    }
    let out = state.join(format!(
        "{stage}-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    let mut command = vec![
        "registry-release".into(),
        "--mode".into(),
        mode.into(),
        "--tag".into(),
        tag.clone(),
        "--out".into(),
        out.to_string_lossy().into_owned(),
        "--compact".into(),
    ];
    if stage == "publish" {
        command.extend(["--confirmation".into(), format!("publish {commit}")]);
    }
    println!("Release: {tag}\n");
    let result = cli(root, &Options(&command));
    if out.join("report.json").exists() {
        let report = read_json(out.join("report.json"))?;
        print!("{}", stage_summary(&report, stage));
        if result.is_ok() {
            write_json(state.join(format!("{stage}.json")), &report)?;
        }
        if result.is_err() {
            eprintln!("Details: {}", out.join("report.json").display());
        }
    }
    result
}

fn stage_summary(report: &Json, stage: &str) -> String {
    let mut text = format!("{:<25} {:<10} STATUS\n", "CRATE", "VERSION");
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for row in rows(&report["packages"]) {
        let (label, detail) = match str_at(row, "status") {
            "already-published" => ("published", String::new()),
            "published-verified" => ("uploaded", String::new()),
            "publish-needed" if stage == "preflight" => ("ready", String::new()),
            "verified-not-uploaded" => ("passed", String::new()),
            "blocked-unpublished-dependencies" => (
                "waiting",
                format!(
                    " — {}",
                    rows(&row["blocked_by"])
                        .iter()
                        .filter_map(Json::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ),
            "version-conflict" => ("conflict", " — bump version".into()),
            _ => ("unchecked", String::new()),
        };
        *counts.entry(label).or_default() += 1;
        text.push_str(&format!(
            "{:<25} {:<10} {label}{detail}\n",
            str_at(row, "name"),
            str_at(row, "version")
        ));
    }
    let summary = counts
        .iter()
        .map(|(label, count)| format!("{count} {label}"))
        .collect::<Vec<_>>()
        .join(", ");
    let failed = report["status"] == "failed"
        || counts.contains_key("conflict")
        || counts.contains_key("unchecked");
    text.push_str(&format!(
        "\n{stage}: {} — {summary}\n",
        if failed { "FAILED" } else { "PASS" }
    ));
    if counts.contains_key("waiting") {
        text.push_str("Waiting crates are checked again during publication, after their release dependencies upload.\n");
    }
    text
}

fn publication_summary(report: &Json) -> String {
    let mut summary =
        String::from("Crate publication checks (no upload approval or ownership check implied)\n");
    let mut ready = 0;
    let mut published = 0;
    let mut blocked = 0;
    for row in rows(&report["packages"]) {
        let status = str_at(row, "status");
        let label = match status {
            "verified-not-uploaded" => {
                ready += 1;
                "READY: package/build checks passed".to_owned()
            }
            "already-published" | "published-verified" => {
                published += 1;
                "PUBLISHED: matching registry payload; skip".to_owned()
            }
            "blocked-unpublished-dependencies" => {
                blocked += 1;
                let dependencies = rows(&row["blocked_by"])
                    .iter()
                    .filter_map(Json::as_str)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("BLOCKED: publish dependencies first: {dependencies}")
            }
            "version-conflict" => {
                blocked += 1;
                "BLOCKED: published version has different content; bump version".to_owned()
            }
            "publish-needed" => {
                blocked += 1;
                "NOT VERIFIED: version available; package/build checks outstanding".to_owned()
            }
            _ => {
                blocked += 1;
                format!(
                    "NOT VERIFIED: {}",
                    if status.is_empty() {
                        "checks did not complete"
                    } else {
                        status
                    }
                )
            }
        };
        summary.push_str(&format!(
            "{:<25} {:<10} {label}\n",
            str_at(row, "name"),
            str_at(row, "version")
        ));
    }
    summary.push_str(&format!("\nCan publish all remaining crates now? {}\n{ready} ready, {published} published, {blocked} blocked or unverified.\n",
        if report["error"].is_string() { "NO — checks failed" }
        else if blocked > 0 { "NO — see per-crate blockers" }
        else if ready == 0 { "Nothing to publish" }
        else { "Package/build checks passed; registry permissions and release approval still required" }));
    if let Some(error) = report["error"].as_str() {
        summary.push_str(&format!("Error: {error}\n"));
    }
    summary
}
pub(super) fn prepare(root: &Path, o: &Options) -> Result<()> {
    let out = outside(root, Path::new(o.required("--out")?))?;
    ensure!(!out.exists(), "Output already exists");
    let mut plan = inventory::plan(root)?;
    plan["source_commit"] = json!(git(root, &["rev-parse", "HEAD"])?);
    plan["source_dirty"] = json!(!git(root, &["status", "--porcelain"])?.is_empty());
    fs::create_dir_all(&out)?;
    let result = (|| {
        if o.has("--archives") {
            for row in rows(&plan["packages"]) {
                let manifest = root.join(str_at(row, "manifest"));
                copy_tree(
                    manifest.parent().unwrap(),
                    &out.join("staged")
                        .join(Path::new(str_at(row, "manifest")).parent().unwrap()),
                )?;
            }
            for row in plan["packages"].as_array_mut().unwrap() {
                let manifest = out.join("staged").join(str_at(row, "manifest"));
                let (code, _) = execute(
                    &out,
                    row,
                    &[
                        "package".into(),
                        "--offline".into(),
                        "--no-verify".into(),
                        "--exclude-lockfile".into(),
                        "--manifest-path".into(),
                        manifest.to_string_lossy().into_owned(),
                    ],
                    None,
                )?;
                ensure!(code == 0, "Offline archive generation failed");
                let id = format!("{}-{}", str_at(row, "name"), str_at(row, "version"));
                let archive = out.join("target/package").join(format!("{id}.crate"));
                let files = verify::inspect_archive(
                    &archive,
                    manifest.parent().unwrap(),
                    &out.join("unpacked"),
                    &id,
                )?;
                row["archive"] = json!({"path":archive,"sha256":sha(&fs::read(&archive)?),"files":files.len(),"inspection":"passed"});
            }
            plan["archive_scope"] = json!(
                "Offline content inspection only; no registry resolution or Cargo build verification."
            );
        }
        Ok(())
    })();
    plan["preparation_status"] = json!(if result.is_ok() { "passed" } else { "failed" });
    if let Err(e) = &result {
        plan["error"] = json!(format!("{e:#}"));
    }
    write_json(out.join("release-plan.json"), &plan)?;
    result
}
#[cfg(test)]
pub(super) mod tests {
    use super::*;
    pub(crate) fn archive(name: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
        let enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut tar = tar::Builder::new(enc);
        for (path, data) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, format!("{name}/{path}"), *data)
                .unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap()
    }
    #[test]
    fn payload_ignores_generated_state_not_code() {
        let manifest = b"[package]\nname='sample'\nversion='1.0.0'\n";
        let a = archive(
            "sample-1.0.0",
            &[
                ("Cargo.toml", manifest),
                ("Cargo.lock", b"old"),
                ("src/lib.rs", b"code"),
            ],
        );
        let b = archive(
            "sample-1.0.0",
            &[
                ("Cargo.toml", manifest),
                ("Cargo.lock", b"new"),
                ("src/lib.rs", b"code"),
            ],
        );
        assert_eq!(
            payload(&a, "sample", "1.0.0").unwrap(),
            payload(&b, "sample", "1.0.0").unwrap()
        );
        assert!(payload(&a, "other", "1.0.0").is_err());
        for (manifest, code) in [
            (manifest.as_slice(), b"changed code".as_slice()),
            (
                b"[package]\nname='sample'\nversion='1.0.0'\n[dependencies]\nother='2'\n"
                    .as_slice(),
                b"code".as_slice(),
            ),
        ] {
            let changed = archive(
                "sample-1.0.0",
                &[("Cargo.toml", manifest), ("src/lib.rs", code)],
            );
            assert_ne!(
                payload(&a, "sample", "1.0.0").unwrap(),
                payload(&changed, "sample", "1.0.0").unwrap()
            );
        }
    }
    #[derive(Default)]
    struct Fake {
        existing: BTreeSet<String>,
        conflict: bool,
        verified: Vec<String>,
        uploaded: Vec<String>,
    }
    impl Driver for Fake {
        fn package(&mut self, _: &mut Json) -> Result<()> {
            Ok(())
        }
        fn existing(&mut self, n: &str, _: &str) -> Result<Option<Json>> {
            Ok(self
                .existing
                .contains(n)
                .then(|| json!({"checksum":"verified"})))
        }
        fn compare(&mut self, _: &Json, _: &Json) -> Result<Vec<String>> {
            Ok(if self.conflict {
                vec!["src/lib.rs".into()]
            } else {
                vec![]
            })
        }
        fn verify(&mut self, r: &mut Json) -> Result<()> {
            self.verified.push(str_at(r, "name").into());
            Ok(())
        }
        fn upload(&mut self, r: &mut Json) -> Result<()> {
            self.uploaded.push(str_at(r, "name").into());
            Ok(())
        }
        fn save(&mut self, _: &Json) -> Result<()> {
            Ok(())
        }
    }
    fn report(mode: &str) -> Json {
        json!({"mode":mode,"release_ready":false,"packages":[{"name":"parent","version":"1.0.0","dependencies":[],"publish_policy":true},{"name":"child","version":"1.0.0","dependencies":["parent"],"publish_policy":true}]})
    }
    #[test]
    fn collisions_block_all_uploads() {
        let mut f = Fake {
            existing: BTreeSet::from(["child".into()]),
            conflict: true,
            ..Default::default()
        };
        assert!(transition(&mut report("publish"), &mut f).is_err());
        assert!(f.verified.is_empty() && f.uploaded.is_empty());
    }
    #[test]
    fn plan_and_dry_run_never_upload() {
        let mut f = Fake::default();
        transition(&mut report("plan"), &mut f).unwrap();
        assert!(f.verified.is_empty() && f.uploaded.is_empty());
        let mut r = report("dry-run");
        transition(&mut r, &mut f).unwrap();
        assert_eq!(f.verified, vec!["parent"]);
        assert!(f.uploaded.is_empty());
        assert_eq!(
            r["packages"][1]["status"],
            "blocked-unpublished-dependencies"
        );
    }
    #[test]
    fn publish_dependency_order_and_skip_existing() {
        let mut f = Fake::default();
        let mut r = report("publish");
        transition(&mut r, &mut f).unwrap();
        assert_eq!(f.uploaded, vec!["parent", "child"]);
        assert_eq!(r["release_ready"], true);
        let mut f = Fake {
            existing: BTreeSet::from(["parent".into(), "child".into()]),
            ..Default::default()
        };
        let mut r = report("publish");
        for row in r["packages"].as_array_mut().unwrap() {
            row["publish_policy"] = json!(false);
        }
        transition(&mut r, &mut f).unwrap();
        assert!(f.uploaded.is_empty() && f.verified.is_empty());
    }
    #[test]
    fn disabled_prevents_every_upload() {
        let mut f = Fake::default();
        let mut r = report("publish");
        r["packages"][1]["publish_policy"] = json!(false);
        assert!(transition(&mut r, &mut f).is_err());
        assert!(f.uploaded.is_empty());
    }
    #[test]
    fn exact_confirmation_before_upload() {
        for (token, confirmation) in [
            ("", "publish sha"),
            ("secret", ""),
            ("secret", "publish other"),
        ] {
            assert!(publication_guard("publish", confirmation, "sha", token).is_err());
        }
        assert!(publication_guard("plan", "", "sha", "").is_ok());
    }
    #[test]
    fn archive_rejects_links_and_duplicates() {
        let duplicate = archive("sample", &[("a", b"one"), ("a", b"two")]);
        assert!(archive_contents(&duplicate).is_err());
    }
}
#[cfg(test)]
mod retry_regressions {
    use super::*;
    #[test]
    fn release_stages_require_successful_evidence_for_the_same_commit() {
        assert!(
            require_stage(
                &json!({"source_commit":"one","status":"planned"}),
                "one",
                "preflight"
            )
            .is_ok()
        );
        assert!(
            require_stage(
                &json!({"source_commit":"one","status":"dry-run-complete"}),
                "one",
                "check"
            )
            .is_ok()
        );
        for report in [
            json!({"source_commit":"other","status":"dry-run-complete"}),
            json!({"source_commit":"one","status":"failed"}),
            json!({"source_commit":"one","status":"planned"}),
        ] {
            assert!(require_stage(&report, "one", "check").is_err());
        }
    }
    #[test]
    fn stage_tables_distinguish_available_versions_from_checked_builds() {
        let mut report = json!({"status":"planned","packages":[
            {"name":"core","version":"2.0.0","status":"publish-needed"},
            {"name":"extension","version":"1.0.0","status":"already-published"}
        ]});
        let preflight = stage_summary(&report, "preflight");
        assert!(preflight.contains("preflight: PASS — 1 published, 1 ready"));
        assert!(stage_summary(&report, "check").contains("check: FAILED"));
        report["packages"][0]["status"] = json!("blocked-unpublished-dependencies");
        report["packages"][0]["blocked_by"] = json!(["macros"]);
        let check = stage_summary(&report, "check");
        assert!(check.contains("waiting — macros"));
        assert!(check.contains("checked again during publication"));
    }
    #[test]
    fn publication_summary_includes_every_crate_and_names_blockers() {
        let report = json!({"packages":[
            {"name":"new","version":"1.0.0","status":"verified-not-uploaded"},
            {"name":"existing","version":"1.0.0","status":"already-published"},
            {"name":"dependent","version":"1.0.0","status":"blocked-unpublished-dependencies","blocked_by":["new"]},
            {"name":"conflict","version":"1.0.0","status":"version-conflict"},
            {"name":"untested","version":"1.0.0","status":"publish-needed"}
        ]});
        let summary = publication_summary(&report);
        assert!(summary.contains("NO — see per-crate blockers"));
        assert!(summary.contains("publish dependencies first: new"));
        assert!(summary.contains("1 ready, 1 published, 3 blocked or unverified"));
        assert!(summary.contains("bump version"));
        assert!(summary.contains("NOT VERIFIED: version available"));
    }
    #[test]
    fn publication_summary_does_not_claim_failed_checks_are_ready() {
        let report = json!({"error":"package failed","packages":[
            {"name":"new","version":"1.0.0"}
        ]});
        assert!(publication_summary(&report).contains("NO — checks failed"));
        assert!(publication_summary(&report).contains("checks did not complete"));
        let published = json!({"packages":[{"name":"existing","version":"1.0.0","status":"already-published"}]});
        assert!(publication_summary(&published).contains("Nothing to publish"));
    }
    #[test]
    fn saved_logs_are_bounded_and_preserve_utf8_diagnostic_tails() {
        let short = "error: failed to compile\n";
        assert_eq!(compact_log(short), short);
        let text = format!("{}\n{short}", "🙂".repeat(40_000));
        let saved = compact_log(&text);
        assert!(saved.len() <= 64 * 1024);
        assert!(saved.starts_with("[Earlier output omitted"));
        assert!(saved.ends_with(short));
    }
    #[test]
    fn quota_retries_require_http_429_and_explicit_gmt_deadline() {
        let date = "Sat, 07 Nov 2026 20:00:00 GMT";
        assert_eq!(
            retry_deadline(&format!("HTTP 429: try again after {date}")),
            Some(httpdate::parse_http_date(date).unwrap())
        );
        assert!(retry_deadline(&format!("try again after {date}")).is_none());
        assert!(retry_deadline("HTTP 429: try again later").is_none());
        assert!(retry_deadline("HTTP 429: try again after invalid GMT").is_none());
    }
}
