//! Release commands. Network and upload operations are explicit CLI actions;
//! unit tests exercise policy and state transitions without registry access.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
mod changes;
mod daily;
mod inventory;
mod registry;
#[cfg(test)]
mod test_fixture;
mod upstream;
mod verify;
pub(super) fn command(root: &Path, args: &[String]) -> Result<()> {
    let options = Options(args);
    match args.first().map(String::as_str) {
        Some("preflight") => print_json(&inventory::audit(root)?),
        Some("prepare-registry") => registry::prepare(root, &options),
        Some("registry-release") => registry::cli(root, &options),
        Some("release-flow") => registry::flow(root, args),
        Some("release-check") => changes::cli(root, &options),
        Some("daily") => daily::cli(root, &options),
        Some("import" | "review" | "context" | "review-notes") => upstream::cli(root, args),
        Some("verify-packages" | "verify-color") => verify::cli(root, &options),
        Some("check-local-dependencies") => verify::check_local(root, &options),
        _ => bail!(
            "Usage: xgpui-release normalize|build-manifests|preflight|prepare-registry|registry-release|release-check|daily|import|review|context|review-notes|verify-packages|verify-color|check-local-dependencies [options]"
        ),
    }
}
struct Options<'a>(&'a [String]);
impl Options<'_> {
    fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .position(|s| s == name)
            .and_then(|i| self.0.get(i + 1))
            .map(String::as_str)
    }
    fn required(&self, name: &str) -> Result<&str> {
        self.get(name).with_context(|| format!("Missing {name}"))
    }
    fn has(&self, name: &str) -> bool {
        self.0.iter().any(|s| s == name)
    }
}
fn read_json(path: impl AsRef<Path>) -> Result<Json> {
    Ok(serde_json::from_slice(
        &fs::read(path.as_ref()).with_context(|| format!("Read {}", path.as_ref().display()))?,
    )?)
}
fn read_toml(path: impl AsRef<Path>) -> Result<Json> {
    parse_toml(&fs::read_to_string(path)?)
}
fn parse_toml(text: &str) -> Result<Json> {
    Ok(serde_json::to_value(toml::from_str::<toml::Value>(text)?)?)
}
fn str_at<'a>(value: &'a Json, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}
fn rows(value: &Json) -> &[Json] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn strings(value: &Json) -> BTreeSet<String> {
    rows(value)
        .iter()
        .filter_map(Json::as_str)
        .map(str::to_owned)
        .collect()
}
fn write_json(path: impl AsRef<Path>, value: &Json) -> Result<()> {
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(value)?))?;
    Ok(())
}
fn print_json(value: &Json) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
fn sha(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn output(root: &Path, program: &str, args: &[&str]) -> Result<String> {
    let result = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("Start {program}"))?;
    ensure!(
        result.status.success(),
        "{program} {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}
fn git(root: &Path, args: &[&str]) -> Result<String> {
    output(root, "git", args)
}
fn checked(command: &mut Command) -> Result<()> {
    let status = command.status()?;
    ensure!(status.success(), "Command failed ({status}): {command:?}");
    Ok(())
}
fn now() -> String {
    // ISO UTC via a small civil-date conversion, no platform date utility.
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let z = (seconds / 86400) as i64 + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    y += i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+00:00",
        seconds % 86400 / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}
fn absolute(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return Ok(path.canonicalize()?);
    }
    let parent = path.parent().context("Missing parent")?;
    let parent = if parent.as_os_str().is_empty() {
        std::env::current_dir()?
    } else {
        absolute(parent)?
    };
    Ok(parent.join(path.file_name().context("Missing filename")?))
}
fn outside(root: &Path, path: &Path) -> Result<PathBuf> {
    let path = absolute(path)?;
    ensure!(
        !path.starts_with(root.canonicalize()?),
        "Output must be outside the checkout"
    );
    Ok(path)
}
fn safe_relative(path: &Path) -> Result<()> {
    ensure!(
        !path.as_os_str().is_empty()
            && path.components().all(|c| matches!(c, Component::Normal(_))),
        "Unsafe path {}",
        path.display()
    );
    Ok(())
}
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in WalkDir::new(from)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            !matches!(
                e.file_name().to_str(),
                Some("target" | "Cargo.lock" | ".cargo" | ".git")
            )
        })
    {
        let entry = entry?;
        let dest = to.join(entry.path().strip_prefix(from)?);
        if entry.file_type().is_dir() {
            fs::create_dir_all(dest)?;
        } else if entry.file_type().is_symlink() {
            bail!("Review symlink before staging: {}", entry.path().display());
        } else {
            fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}
fn fetch(url: &str, missing_ok: bool) -> Result<Option<Vec<u8>>> {
    ensure!(url.starts_with("https://"), "HTTPS required");
    let file = tempfile::NamedTempFile::new()?;
    let status = output(
        &std::env::current_dir()?,
        "curl",
        &[
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            "30",
            "--user-agent",
            "xgpui-release (https://github.com/gpui-luma-dev/xgpui)",
            "--output",
            file.path().to_str().context("Non-UTF8 scratch path")?,
            "--write-out",
            "%{http_code}",
            url,
        ],
    )?;
    if status == "404" && missing_ok {
        return Ok(None);
    }
    ensure!(status == "200", "HTTP {status}: {url}");
    Ok(Some(fs::read(file.path())?))
}
fn fetch_json(url: &str, missing_ok: bool) -> Result<Option<Json>> {
    fetch(url, missing_ok)?
        .map(|b| Ok(serde_json::from_slice(&b)?))
        .transpose()
}
fn allowed(policy: &Json) -> bool {
    policy != &json!(false) && (!policy.is_array() || rows(policy).contains(&json!("crates-io")))
}
