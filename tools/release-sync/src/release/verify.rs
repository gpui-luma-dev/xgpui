use super::*;
pub(super) fn inspect_archive(
    archive: &Path,
    staged: &Path,
    destination: &Path,
    id: &str,
) -> Result<Vec<String>> {
    let files = registry::archive_contents(&fs::read(archive)?)?;
    let prefix = format!("{id}/");
    ensure!(
        files.keys().all(|k| k.starts_with(&prefix)),
        "Unexpected archive root"
    );
    let mut required = BTreeSet::from([
        "README.md".to_owned(),
        "LICENSE-APACHE".into(),
        "Cargo.toml.orig".into(),
    ]);
    let suffixes = [
        "rs", "wgsl", "metal", "hlsl", "h", "ttf", "otf", "c", "cc", "cpp", "m", "mm", "rc", "xml",
        "png", "jpg", "jpeg", "svg", "bin", "spv",
    ];
    for entry in WalkDir::new(staged).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(staged)?;
        let nested = relative
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
            .any(|p| staged.join(p).join("Cargo.toml").is_file());
        if nested {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if suffixes.contains(
            &entry
                .path()
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or(""),
        ) || name.starts_with("LICENSE")
            || name.starts_with("NOTICE")
        {
            required.insert(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    for relative in required {
        let original = staged.join(if relative == "Cargo.toml.orig" {
            "Cargo.toml"
        } else {
            &relative
        });
        let data = files
            .get(&format!("{id}/{relative}"))
            .with_context(|| format!("Archive omits required input: {relative}"))?;
        ensure!(
            fs::read(original)? == *data,
            "Archive content mismatch: {relative}"
        );
    }
    let doc = parse_toml(std::str::from_utf8(
        files
            .get(&format!("{id}/Cargo.toml"))
            .context("Missing archived manifest")?,
    )?)?;
    ensure!(
        doc.get("patch").is_none() && doc.get("replace").is_none(),
        "Unexpected archived dependency overrides"
    );
    ensure!(
        inventory::dependencies(&doc)
            .iter()
            .all(|d| d.get("path").is_none()),
        "Unnormalized path dependency in archive"
    );
    for (name, bytes) in &files {
        let path = destination.join(name);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, bytes)?;
    }
    Ok(files.into_keys().collect())
}
fn api(name: &str) -> Result<&'static str> {
    Ok(match name {
        "xgpui" => {
            "pub fn size() -> gpui::Size<gpui::Pixels> { gpui::size(gpui::px(320.), gpui::px(240.)) }"
        }
        "xgpui-macros" => "pub use gpui_macros::Action;",
        "xgpui-apple" => "pub use gpui_apple::AppleDispatcher;",
        "xgpui-macos" => "pub use gpui_macos::MacPlatform;",
        "xgpui-ios" => "pub use gpui_ios::{IosPlatform, StatusBarContentStyle};",
        "xgpui-windows" => "pub use gpui_windows::WindowsPlatform;",
        "xgpui-linux" => "pub use gpui_linux::current_platform;",
        "xgpui-web" => "pub use gpui_web::{WebPlatform, WebBackendPreference};",
        "xgpui-platform" => "pub use gpui_platform::{application, current_platform};",
        "xgpui-wgpu" => {
            "pub fn features() -> gpui_wgpu::wgpu::Features { gpui_wgpu::wgpu::Features::empty() }"
        }
        "xgpui-color" => "pub use xgpui_color::{EncodedRgb, RgbSpace, WGSL_COLOR_MATH};",
        "xgpui-shader" => "pub use xgpui_shader::{painted, Paint};",
        "xgpui-gamut" => "pub use xgpui_gamut::WideColor;",
        "xgpui-media" => {
            "pub fn filter() -> xgpui_media::ImageFilter { xgpui_media::ImageFilter::new().brightness(1.1) }"
        }
        "xgpui-parley" => "pub use xgpui_parley::ParleyTextSystem;",
        "xgpui-svg" => {
            "pub fn scheduler() -> xgpui_svg::SvgScheduler { xgpui_svg::SvgScheduler::default() }"
        }
        "xgpui-canvas" => "pub use xgpui_canvas::{Camera2D, Scene};",
        _ => bail!("Add a consumer API smoke check for {name}"),
    })
}
fn write_toml(path: &Path, doc: &Json) -> Result<()> {
    let value: toml::Value = serde_json::from_value(doc.clone())?;
    let text = toml::to_string(&value)?;
    ensure!(
        parse_toml(&text)? == *doc,
        "Scratch TOML serialization changed values"
    );
    fs::write(path, text)?;
    Ok(())
}
fn rewire(doc: &mut Json, candidates: &BTreeMap<String, PathBuf>) -> Result<Vec<Json>> {
    let mut rewrites = Vec::new();
    fn table(
        doc: &mut Json,
        candidates: &BTreeMap<String, PathBuf>,
        rewrites: &mut Vec<Json>,
    ) -> Result<()> {
        for kind in ["dependencies", "build-dependencies", "dev-dependencies"] {
            if let Some(deps) = doc[kind].as_object_mut() {
                for (alias, spec) in deps {
                    let mut value = if spec.is_string() {
                        json!({"version":spec})
                    } else {
                        spec.clone()
                    };
                    let name = value["package"].as_str().unwrap_or(alias).to_owned();
                    if let Some(path) = candidates.get(&name) {
                        ensure!(
                            value.get("version").is_some(),
                            "Candidate {name} has no registry version"
                        );
                        value["path"] = json!(path);
                        *spec = value;
                        rewrites.push(json!({"alias":alias,"package":name,"kind":kind}));
                    }
                }
            }
        }
        Ok(())
    }
    table(doc, candidates, &mut rewrites)?;
    if let Some(targets) = doc["target"].as_object_mut() {
        for target in targets.values_mut() {
            table(target, candidates, &mut rewrites)?;
        }
    }
    Ok(rewrites)
}
struct Runner<'a> {
    out: &'a Path,
    offline: bool,
    attempt: usize,
}
impl Runner<'_> {
    fn run(
        &self,
        report: &mut Json,
        label: &str,
        args: &[String],
        nightly: bool,
        flags: Option<&str>,
    ) -> Result<(bool, PathBuf)> {
        let log = self
            .out
            .join("logs")
            .join(format!("{label}-attempt{}.log", self.attempt));
        fs::create_dir_all(log.parent().unwrap())?;
        let file = fs::File::create(&log)?;
        let mut command = registry::isolated_command(self.out, "cargo")?;
        if nightly {
            command
                .arg("+nightly")
                .env("CARGO_TARGET_DIR", self.out.join("target-nightly"));
        }
        command.arg(&args[0]);
        if self.offline {
            command.arg("--offline");
        }
        command
            .args(&args[1..])
            .stdout(file.try_clone()?)
            .stderr(file);
        if let Some(flags) = flags {
            command.env("RUSTFLAGS", flags);
        }
        println!("{label}");
        let status = command.status()?;
        if report.get("steps").is_none() {
            report["steps"] = json!([]);
        }
        report["steps"].as_array_mut().unwrap().push(json!({"name":label,"command":format!("{command:?}"),"exit_code":status.code(),"log":log,"rustflags":flags}));
        write_json(self.out.join("report.json"), report)?;
        Ok((status.success(), log))
    }
}
fn target<'a>(name: &str, host: &'a str) -> &'a str {
    match name {
        "xgpui-windows" => "x86_64-pc-windows-msvc",
        "xgpui-linux" => "x86_64-unknown-linux-gnu",
        "xgpui-web" => "wasm32-unknown-unknown",
        "xgpui-ios" => "aarch64-apple-ios-sim",
        _ => host,
    }
}
fn validate_metadata(
    root: &Path,
    metadata: &Json,
    candidates: &BTreeMap<String, PathBuf>,
) -> Result<()> {
    for p in rows(&metadata["packages"]) {
        let path = absolute(Path::new(str_at(p, "manifest_path")))?;
        ensure!(
            !path.starts_with(root),
            "Consumer resolved a checkout package"
        );
        if let Some(expected) = candidates.get(str_at(p, "name")) {
            ensure!(
                path == expected.join("Cargo.toml") && p["source"].is_null(),
                "Consumer resolved candidate outside extracted validation copies"
            );
        }
    }
    Ok(())
}
fn nightly_flags(root: &Path) -> Result<String> {
    let doc = read_toml(root.join("gpui/crates/gpui-web/examples/hello_web/.cargo/config.toml"))?;
    Ok(rows(&doc["target"]["wasm32-unknown-unknown"]["rustflags"])
        .iter()
        .map(|v| v.as_str().unwrap_or(""))
        .collect::<Vec<_>>()
        .join(" "))
}
fn verify_packages(root: &Path, out: &Path, o: &Options) -> Result<Json> {
    let retry = o.has("--retry-consumers");
    ensure!(
        !o.has("--profiles-only") || retry,
        "--profiles-only requires --retry-consumers"
    );
    ensure!(retry || !out.exists(), "Output already exists");
    fs::create_dir_all(out)?;
    let mut report = if retry {
        read_json(out.join("report.json"))?
    } else {
        json!({"started_at":now(),"packages":{},"steps":[],"scope":"archive inspection and target-specific consumer checks; not registry publication"})
    };
    if retry {
        if report.get("previous_consumer_results").is_none() {
            report["previous_consumer_results"] = json!([]);
        }
        let old = report["packages"].clone();
        report["previous_consumer_results"]
            .as_array_mut()
            .unwrap()
            .push(old);
    }
    let runner = Runner {
        out,
        offline: o.has("--offline"),
        attempt: rows(&report["previous_consumer_results"]).len() + 1,
    };
    let rustc = output(root, "rustc", &["-vV"])?;
    let host = rustc
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .context("Missing rustc host")?
        .to_owned();
    report["rustc"] = json!(rustc);
    report["cargo"] = json!(output(root, "cargo", &["--version"])?);
    let manifests = inventory::manifests(root)?;
    let mut candidates = BTreeMap::new();
    for (name, manifest) in &manifests {
        let staged = out
            .join("staged")
            .join(Path::new(manifest).parent().unwrap());
        if !retry {
            copy_tree(root.join(manifest).parent().unwrap(), &staged)?;
            let doc = read_toml(staged.join("Cargo.toml"))?;
            report["packages"][name] =
                json!({"version":doc["package"]["version"],"consumer_status":"not-run"});
            let (ok, log) = runner.run(
                &mut report,
                &format!("package-{name}"),
                &[
                    "package".into(),
                    "--no-verify".into(),
                    "--exclude-lockfile".into(),
                    "--manifest-path".into(),
                    staged.join("Cargo.toml").to_string_lossy().into_owned(),
                ],
                false,
                None,
            )?;
            if !ok {
                report["packages"][name]["archive_status"] = json!("failed");
                report["packages"][name]["log"] = json!(log);
                continue;
            }
            let id = format!("{name}-{}", str_at(&report["packages"][name], "version"));
            let archive = out.join("target/package").join(format!("{id}.crate"));
            match inspect_archive(&archive, &staged, &out.join("unpacked"), &id) {
                Ok(files) => {
                    let candidate = out.join("validation-copies").join(name);
                    copy_tree(&out.join("unpacked").join(&id), &candidate)?;
                    report["packages"][name]["archive_status"] = json!("passed");
                    report["packages"][name]["archive"] = json!(archive);
                    report["packages"][name]["archive_sha256"] = json!(sha(&fs::read(&archive)?));
                    report["packages"][name]["files"] = json!(files);
                }
                Err(e) => {
                    report["packages"][name]["archive_status"] = json!("failed");
                    report["packages"][name]["error"] = json!(format!("{e:#}"));
                }
            }
        }
        if report["packages"][name]["archive_status"] == "passed" {
            candidates.insert(name.clone(), out.join("validation-copies").join(name));
        }
        write_json(out.join("report.json"), &report)?;
    }
    for (name, path) in &candidates {
        let manifest = path.join("Cargo.toml");
        let mut doc = read_toml(&manifest)?;
        report["packages"][name]["validation_only_rewrites"] =
            json!(rewire(&mut doc, &candidates)?);
        write_toml(&manifest, &doc)?;
    }
    for (name, path) in &candidates {
        if o.has("--profiles-only") {
            continue;
        }
        if retry
            && report["packages"][name]["consumer_status"] == "passed"
            && report["packages"][name]["dependency_metadata_status"] == "passed"
        {
            continue;
        }
        if name == "xgpui-color" {
            report["packages"][name]["consumer_status"] =
                json!("covered-by-dedicated-color-validator");
            continue;
        }
        let dir = out.join("consumers").join(name);
        fs::create_dir_all(dir.join("src"))?;
        let doc = read_toml(path.join("Cargo.toml"))?;
        let alias = doc["lib"]["name"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| name.replace('-', "_"));
        let mut dep = json!({"package":name,"version":format!("={}",str_at(&report["packages"][name],"version")),"path":path});
        if name == "xgpui-ios" {
            dep["features"] = json!(["runtime_shaders"]);
        }
        let manifest = dir.join("Cargo.toml");
        write_toml(
            &manifest,
            &json!({"package":{"name":format!("consumer-{name}"),"version":"0.0.0","edition":"2024","publish":false},"workspace":{},"dependencies":{alias:dep}}),
        )?;
        fs::write(dir.join("src/lib.rs"), api(name)?)?;
        let nightly = name == "xgpui-web" && o.has("--nightly-web");
        let flags = if nightly {
            Some(nightly_flags(root)?)
        } else {
            None
        };
        let mut args = vec!["check".into()];
        if nightly {
            args.extend(["-Z".into(), "build-std=std,panic_abort".into()]);
        }
        args.extend([
            "--jobs".into(),
            "4".into(),
            "--target".into(),
            target(name, &host).into(),
            "--manifest-path".into(),
            manifest.to_string_lossy().into_owned(),
        ]);
        let (ok, log) = runner.run(
            &mut report,
            &format!("consumer-{name}"),
            &args,
            nightly,
            flags.as_deref(),
        )?;
        report["packages"][name]["consumer_status"] = json!(if ok { "passed" } else { "failed" });
        report["packages"][name]["consumer_log"] = json!(log);
        report["packages"][name]["target"] = json!(target(name, &host));
        let (ok, log) = runner.run(
            &mut report,
            &format!("metadata-{name}"),
            &[
                "metadata".into(),
                "--format-version".into(),
                "1".into(),
                "--manifest-path".into(),
                manifest.to_string_lossy().into_owned(),
            ],
            false,
            None,
        )?;
        let result = (|| {
            ensure!(ok, "Metadata command failed");
            let text = fs::read_to_string(&log)?;
            let start = text
                .find("{\"packages\":")
                .context("Missing Cargo metadata JSON")?;
            let mut stream = serde_json::Deserializer::from_str(&text[start..]).into_iter::<Json>();
            let metadata = stream.next().context("Missing metadata")??;
            validate_metadata(root, &metadata, &candidates)
        })();
        report["packages"][name]["dependency_metadata_status"] =
            json!(if result.is_ok() { "passed" } else { "failed" });
        if let Err(e) = result {
            report["packages"][name]["dependency_metadata_error"] = json!(format!("{e:#}"));
            report["packages"][name]["consumer_status"] = json!("failed");
        }
    }
    let profiles = vec![
        (
            "xgpui-apple",
            "font-kit",
            vec!["font-kit"],
            true,
            "pub use gpui_apple::AppleTextSystem;",
        ),
        (
            "xgpui-web",
            "single-threaded",
            vec![],
            false,
            api("xgpui-web")?,
        ),
        (
            "xgpui-web",
            "single-threaded-application",
            vec![],
            false,
            api("xgpui-web")?,
        ),
        (
            "xgpui-shader",
            "portable",
            vec!["portable-shaders"],
            true,
            "pub use xgpui_shader::{Brush, BrushStop, LayerEffect};",
        ),
        (
            "xgpui-gamut",
            "instanced",
            vec!["instanced"],
            true,
            "pub use xgpui_gamut::instanced::{WidePainter, WideGradient};",
        ),
        (
            "xgpui-media",
            "wgpu",
            vec!["wgpu"],
            true,
            api("xgpui-media")?,
        ),
        (
            "xgpui-parley",
            "system-fonts",
            vec!["system-fonts"],
            true,
            api("xgpui-parley")?,
        ),
    ];
    for (name, profile, features, defaults, code) in profiles {
        let Some(path) = candidates.get(name) else {
            continue;
        };
        if retry && report["packages"][name]["feature_profiles"][profile]["status"] == "passed" {
            continue;
        }
        let dir = out.join("feature-consumers").join(name).join(profile);
        fs::create_dir_all(dir.join("src"))?;
        let doc = read_toml(path.join("Cargo.toml"))?;
        let alias = doc["lib"]["name"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| name.replace('-', "_"));
        let mut consumer = json!({"package":{"name":format!("feature-consumer-{name}-{profile}"),"version":"0.0.0","edition":"2024","publish":false},"workspace":{},"dependencies":{alias:{"package":name,"version":format!("={}",str_at(&report["packages"][name],"version")),"path":path,"default-features":defaults,"features":features}}});
        if profile == "single-threaded-application" {
            consumer["dependencies"]["wgpu"] = json!({"version":"=30.0.1","default-features":false,"features":["fragile-send-sync-non-atomic-wasm"]});
        }
        let manifest = dir.join("Cargo.toml");
        write_toml(&manifest, &consumer)?;
        fs::write(dir.join("src/lib.rs"), code)?;
        let (ok, log) = runner.run(
            &mut report,
            &format!("feature-{name}-{profile}"),
            &[
                "check".into(),
                "--jobs".into(),
                "4".into(),
                "--target".into(),
                target(name, &host).into(),
                "--manifest-path".into(),
                manifest.to_string_lossy().into_owned(),
            ],
            false,
            None,
        )?;
        report["packages"][name]["feature_profiles"][profile] = json!({"status":if ok{"passed"}else{"failed"},"features":features,"default_features":defaults,"log":log});
    }
    if o.has("--nightly-web") && candidates.contains_key("xgpui-web") {
        let manifest = out.join("consumers/xgpui-web/Cargo.toml");
        let flags = nightly_flags(root)?;
        let (ok, log) = runner.run(
            &mut report,
            "feature-xgpui-web-multithreaded-nightly",
            &[
                "check".into(),
                "-Z".into(),
                "build-std=std,panic_abort".into(),
                "--jobs".into(),
                "4".into(),
                "--target".into(),
                "wasm32-unknown-unknown".into(),
                "--manifest-path".into(),
                manifest.to_string_lossy().into_owned(),
            ],
            true,
            Some(&flags),
        )?;
        report["packages"]["xgpui-web"]["feature_profiles"]["multithreaded-nightly"] = json!({"status":if ok{"passed"}else{"failed"},"default_features":true,"rustflags":flags,"log":log});
    }
    if host.contains("apple-darwin") {
        let dir = out.join("combined-native-consumer");
        if !dir.exists() {
            copy_tree(&root.join("scripts/fixtures/native-package-consumer"), &dir)?;
        }
        let manifest = dir.join("Cargo.toml");
        let mut doc = read_toml(&manifest)?;
        let rewrites = rewire(&mut doc, &candidates)?;
        write_toml(&manifest, &doc)?;
        let (ok, log) = runner.run(
            &mut report,
            "native-consumer-run",
            &[
                "run".into(),
                "--manifest-path".into(),
                manifest.to_string_lossy().into_owned(),
            ],
            false,
            None,
        )?;
        report["native_execution"] =
            json!({"status":if ok{"passed"}else{"failed"},"log":log,"candidate_rewrites":rewrites});
    }
    let passed = report["packages"].as_object().unwrap().values().all(|r| {
        r["archive_status"] == "passed"
            && matches!(
                str_at(r, "consumer_status"),
                "passed" | "covered-by-dedicated-color-validator"
            )
            && r["feature_profiles"]
                .as_object()
                .is_none_or(|p| p.values().all(|p| p["status"] == "passed"))
    }) && report["native_execution"]["status"] != "failed";
    report["finished_at"] = json!(now());
    report["release_ready"] = json!(false);
    report["status"] = json!(if passed {
        "passed"
    } else {
        "completed-with-blockers"
    });
    write_json(out.join("report.json"), &report)?;
    Ok(report)
}
fn verify_color(root: &Path, out: &Path, o: &Options) -> Result<()> {
    ensure!(!out.exists(), "Output already exists");
    fs::create_dir_all(out)?;
    let runner = Runner {
        out,
        offline: o.has("--offline"),
        attempt: 1,
    };
    let mut report = json!({"started_at":now(),"status":"running","offline":o.has("--offline"),"native_metal_requested":o.has("--native-metal"),"release_ready":false,"steps":[]});
    let result = (|| {
        let source = root.join("extensions/crates/xgpui-color");
        let stage = out.join("source");
        copy_tree(&source, &stage)?;
        ensure!(
            fs::read(stage.join("LICENSE-APACHE"))?
                == fs::read(root.join("gpui/crates/gpui/LICENSE-APACHE"))?,
            "Color license notice must match GPUI"
        );
        let doc = read_toml(stage.join("Cargo.toml"))?;
        ensure!(
            doc["package"]["name"] == "xgpui-color",
            "Expected xgpui-color"
        );
        let (ok, _) = runner.run(
            &mut report,
            "package",
            &[
                "package".into(),
                "--manifest-path".into(),
                stage.join("Cargo.toml").to_string_lossy().into_owned(),
            ],
            false,
            None,
        )?;
        ensure!(ok, "Color package command failed");
        let id = format!("xgpui-color-{}", str_at(&doc["package"], "version"));
        let archive = out.join("target/package").join(format!("{id}.crate"));
        let entries = inspect_archive(&archive, &stage, &out.join("unpacked"), &id)?;
        let extracted = out.join("unpacked").join(&id);
        let packaged = read_toml(extracted.join("Cargo.toml"))?;
        ensure!(
            inventory::dependencies(&packaged)
                .iter()
                .all(|d| d["kind"] == "dev-dependencies"),
            "Unexpected color runtime dependencies"
        );
        let mut args = vec![
            "test".into(),
            "--locked".into(),
            "--manifest-path".into(),
            extracted.join("Cargo.toml").to_string_lossy().into_owned(),
        ];
        if !o.has("--native-metal") {
            args.extend([
                "--".into(),
                "--skip".into(),
                "metal_float32_matches_cpu_reference".into(),
            ]);
        }
        let (ok, _) = runner.run(&mut report, "archive-tests", &args, false, None)?;
        ensure!(ok, "Color archive tests failed");
        let consumer = out.join("consumer");
        copy_tree(&root.join("scripts/fixtures/color-consumer"), &consumer)?;
        let manifest = consumer.join("Cargo.toml");
        let mut doc = read_toml(&manifest)?;
        rewire(
            &mut doc,
            &BTreeMap::from([("xgpui-color".into(), extracted.clone())]),
        )?;
        write_toml(&manifest, &doc)?;
        let (ok, _) = runner.run(
            &mut report,
            "consumer-lock",
            &[
                "generate-lockfile".into(),
                "--manifest-path".into(),
                manifest.to_string_lossy().into_owned(),
            ],
            false,
            None,
        )?;
        ensure!(ok, "Consumer lockfile generation failed");
        let (ok, log) = runner.run(
            &mut report,
            "consumer-metadata",
            &[
                "metadata".into(),
                "--locked".into(),
                "--format-version".into(),
                "1".into(),
                "--manifest-path".into(),
                manifest.to_string_lossy().into_owned(),
            ],
            false,
            None,
        )?;
        ensure!(ok, "Consumer metadata failed");
        let text = fs::read_to_string(log)?;
        let start = text
            .find("{\"packages\":")
            .context("Missing Cargo metadata")?;
        let metadata = serde_json::Deserializer::from_str(&text[start..])
            .into_iter::<Json>()
            .next()
            .context("Missing metadata")??;
        let names: BTreeSet<_> = rows(&metadata["packages"])
            .iter()
            .map(|p| str_at(p, "name").to_owned())
            .collect();
        ensure!(
            names == BTreeSet::from(["xgpui-color".into(), "xgpui-color-consumer-check".into()]),
            "Standalone color consumer unexpectedly resolves extra packages"
        );
        validate_metadata(
            root,
            &metadata,
            &BTreeMap::from([("xgpui-color".into(), extracted)]),
        )?;
        let (ok, _) = runner.run(
            &mut report,
            "consumer-run",
            &[
                "run".into(),
                "--locked".into(),
                "--manifest-path".into(),
                manifest.to_string_lossy().into_owned(),
            ],
            false,
            None,
        )?;
        ensure!(ok, "Color consumer execution failed");
        report["consumer_runtime_packages"] = json!(names);
        report["archive"] = json!(archive);
        report["archive_sha256"] = json!(sha(&fs::read(&archive)?));
        report["archive_files"] = json!(entries);
        report["native_metal_executed"] =
            json!(o.has("--native-metal") && cfg!(target_os = "macos"));
        Ok(())
    })();
    report["status"] = json!(if result.is_ok() { "passed" } else { "failed" });
    if let Err(e) = &result {
        report["error"] = json!(format!("{e:#}"));
    }
    report["finished_at"] = json!(now());
    write_json(out.join("report.json"), &report)?;
    result
}
pub(super) fn cli(root: &Path, o: &Options) -> Result<()> {
    let temp = if o.get("--out").is_none() {
        Some(tempfile::tempdir()?)
    } else {
        None
    };
    let out = outside(
        root,
        &o.get("--out")
            .map(PathBuf::from)
            .unwrap_or_else(|| temp.as_ref().unwrap().path().join("verification")),
    )?;
    if o.0[0] == "verify-color" {
        verify_color(root, &out, o)?
    } else {
        let report = verify_packages(root, &out, o)?;
        ensure!(
            report["status"] == "passed",
            "Package verification completed with blockers; see {}",
            out.join("report.json").display()
        );
    }
    if let Some(temp) = temp {
        let path = temp.keep();
        println!("Verification artifacts: {}", path.display());
    }
    Ok(())
}
pub(super) fn check_local(root: &Path, o: &Options) -> Result<()> {
    let provenance = read_json(root.join("upstream.json"))?;
    let metadata = read_json(o.required("--metadata")?)?;
    let manifests = inventory::manifests(root)?;
    let helpers = provenance["registry_packages"]["packages"]
        .as_object()
        .context("Missing registry helper map")?;
    for p in rows(&metadata["packages"]) {
        let name = str_at(p, "name");
        if let Some(manifest) = manifests.get(name) {
            ensure!(
                p["source"].is_null()
                    && absolute(Path::new(str_at(p, "manifest_path")))? == root.join(manifest),
                "{name} resolved outside xgpui"
            );
            let version = provenance["distribution"]["versions"][name]
                .as_str()
                .or(provenance["distribution"]["version"].as_str())
                .unwrap_or("");
            ensure!(
                p["version"] == version,
                "{name} must use downstream version {version}"
            );
        }
        ensure!(
            !helpers.contains_key(name),
            "{name} must use published helper identity"
        );
        if helpers.values().any(|v| v == name) {
            ensure!(
                p["source"] == "registry+https://github.com/rust-lang/crates.io-index"
                    && p["version"] == provenance["registry_packages"]["version"],
                "{name} must resolve from pinned crates.io release"
            );
        } else {
            ensure!(
                name != "gpui-unofficial" && !name.ends_with("-gpui-unofficial"),
                "Unexpected upstream framework package {name}"
            );
        }
    }
    println!(
        "Resolved framework packages are local; supporting packages use the pinned crates.io release."
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rewiring_preserves_versions_targets_and_features() {
        let mut doc = json!({"dependencies":{"core":{"package":"xgpui","version":"3.0.0","features":["test-support"]},"external":"1"},"target":{"cfg(windows)":{"dev-dependencies":{"core":{"package":"xgpui","version":"=3.0.0"}}}}});
        let rewritten = rewire(
            &mut doc,
            &BTreeMap::from([("xgpui".into(), PathBuf::from("/scratch/core"))]),
        )
        .unwrap();
        assert_eq!(rewritten.len(), 2);
        assert_eq!(doc["dependencies"]["core"]["version"], "3.0.0");
        assert_eq!(doc["dependencies"]["external"], "1");
        assert_eq!(
            doc["target"]["cfg(windows)"]["dev-dependencies"]["core"]["path"],
            "/scratch/core"
        );
    }
    #[test]
    fn scratch_toml_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let doc = json!({"package":{"name":"test","version":"1.0.0"},"bin":[{"name":"test","path":"src/main.rs"}],"target":{"cfg(windows)":{"dependencies":{"dep":{"version":"1","optional":true}}}}});
        let p = dir.path().join("Cargo.toml");
        write_toml(&p, &doc).unwrap();
        assert_eq!(read_toml(p).unwrap(), doc);
    }
}
#[cfg(test)]
mod archive_regressions {
    use super::super::registry::tests::archive;
    use super::*;
    fn fixture(root: &Path) -> PathBuf {
        let stage = root.join("source");
        for (path, data) in [
            ("Cargo.toml", "[package]\nname='fixture'\nversion='1.0.0'\n"),
            ("README.md", "# Fixture"),
            ("LICENSE-APACHE", "license"),
            ("src/lib.rs", "code"),
            ("src/math.wgsl", "shader"),
            (
                "examples/app/Cargo.toml",
                "[package]\nname='nested'\nversion='1.0.0'",
            ),
            ("examples/app/src/main.rs", "nested"),
        ] {
            let p = stage.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, data).unwrap();
        }
        stage
    }
    #[test]
    fn assets_required_nested_package_excluded_and_bytes_compared() {
        let root = tempfile::tempdir().unwrap();
        let stage = fixture(root.path());
        let manifest = fs::read(stage.join("Cargo.toml")).unwrap();
        let files = vec![
            ("Cargo.toml", manifest.as_slice()),
            ("Cargo.toml.orig", manifest.as_slice()),
            ("README.md", b"# Fixture".as_slice()),
            ("LICENSE-APACHE", b"license".as_slice()),
            ("src/lib.rs", b"code".as_slice()),
            ("src/math.wgsl", b"shader".as_slice()),
        ];
        let p = root.path().join("fixture.crate");
        fs::write(&p, archive("fixture-1.0.0", &files)).unwrap();
        assert!(
            inspect_archive(&p, &stage, &root.path().join("unpacked"), "fixture-1.0.0").is_ok()
        );
        for changed in [false, true] {
            let mut files = files.clone();
            if changed {
                files.last_mut().unwrap().1 = b"wrong";
            } else {
                files.pop();
            }
            fs::write(&p, archive("fixture-1.0.0", &files)).unwrap();
            assert!(
                inspect_archive(&p, &stage, &root.path().join("bad"), "fixture-1.0.0").is_err()
            );
        }
    }
    #[test]
    fn archive_paths_and_overrides_are_rejected_before_extraction() {
        let root = tempfile::tempdir().unwrap();
        let stage = fixture(root.path());
        let p = root.path().join("fixture.crate");
        fs::write(&p, archive("other-1.0.0", &[("file", b"data")])).unwrap();
        let destination = root.path().join("unpacked");
        assert!(inspect_archive(&p, &stage, &destination, "fixture-1.0.0").is_err());
        assert!(!destination.exists());
    }
    #[test]
    fn candidate_without_registry_version_rejected() {
        let mut doc = json!({"dependencies":{"core":{"package":"xgpui","path":"../core"}}});
        assert!(
            rewire(
                &mut doc,
                &BTreeMap::from([("xgpui".into(), PathBuf::from("scratch"))])
            )
            .is_err()
        );
    }
}
