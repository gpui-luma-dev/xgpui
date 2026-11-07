//! Cargo diagnostic capture and artifact indexing for the Bash test runner.
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env,
    fs::{self, File},
    io::{self, BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
    thread,
};

/// Cargo to run: `TEST_ALL_CARGO`, or `cargo` from PATH. The runner self-test
/// sets it to a fake Bash script. Windows can only start that through an
/// interpreter, so a program without an `.exe` extension runs under
/// `TEST_ALL_SHELL` there (not a PATH lookup, which can reach WSL's bash).
fn cargo_command() -> Result<Command> {
    let Some(program) = env::var_os("TEST_ALL_CARGO") else {
        return Ok(Command::new("cargo"));
    };
    let is_exe = Path::new(&program)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));
    if cfg!(windows) && !is_exe {
        let shell = env::var_os("TEST_ALL_SHELL")
            .context("TEST_ALL_SHELL must name bash to run a TEST_ALL_CARGO script on Windows")?;
        let mut command = Command::new(shell);
        command.arg(program);
        return Ok(command);
    }
    Ok(Command::new(program))
}

fn capture(dir: &Path, name: &str, args: &[String]) -> Result<i32> {
    fs::create_dir_all(dir)?;
    let split = args.iter().position(|s| s == "--").unwrap_or(args.len());
    if args[..split]
        .iter()
        .any(|s| s.starts_with("--message-format"))
    {
        bail!("message-format is owned by the test runner");
    }
    let mut raw = File::create(dir.join(format!("{name}.jsonl")))?;
    let mut stderr_log = File::create(dir.join(format!("{name}.stderr.log")))?;
    let mut child = cargo_command()?
        .args(&args[..split])
        .arg("--message-format=json")
        .args(&args[split..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stderr = child.stderr.take().context("missing stderr")?;
    let reader = thread::spawn(move || -> io::Result<()> {
        let mut console = io::stderr().lock();
        for line in BufReader::new(stderr).split(b'\n') {
            let line = line?;
            stderr_log.write_all(&line)?;
            stderr_log.write_all(b"\n")?;
            console.write_all(&line)?;
            console.write_all(b"\n")?;
            console.flush()?;
        }
        Ok(())
    });
    let stream_result = (|| -> Result<()> {
        let mut console = io::stdout().lock();
        for line in BufReader::new(child.stdout.take().context("missing stdout")?).split(b'\n') {
            let line = line?;
            match serde_json::from_slice::<Value>(&line) {
                Ok(message) if message.get("reason").is_some() => {
                    raw.write_all(&line)?;
                    raw.write_all(b"\n")?;
                    if message["reason"] == "compiler-message" {
                        if let Some(rendered) = message["message"]["rendered"].as_str() {
                            console.write_all(rendered.as_bytes())?;
                        } else {
                            writeln!(
                                console,
                                "{}",
                                message["message"]["message"].as_str().unwrap_or("")
                            )?;
                        }
                    }
                }
                _ => {
                    console.write_all(&line)?;
                    console.write_all(b"\n")?;
                }
            }
            console.flush()?;
        }
        Ok(())
    })();
    if stream_result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait()?;
    reader
        .join()
        .map_err(|_| anyhow::anyhow!("stderr reader panicked"))??;
    stream_result?;
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        Ok(status.code().unwrap_or(128 + status.signal().unwrap_or(1)))
    }
    #[cfg(not(unix))]
    {
        Ok(status.code().unwrap_or(1))
    }
}

fn index(input: &Path, output: &Path) -> Result<()> {
    let mut out = File::create(output)?;
    for line in BufReader::new(File::open(input)?).lines() {
        let message: Value = serde_json::from_str(&line?)?;
        if message["reason"] != "compiler-artifact" {
            continue;
        }
        let Some(exe) = message["executable"].as_str() else {
            continue;
        };
        let kinds = message["target"]["kind"]
            .as_array()
            .context("missing target kind")?;
        let has = |s: &str| kinds.iter().any(|v| v == s);
        if message["profile"]["test"] != true && !has("bench") {
            continue;
        }
        let kind = if has("bin") {
            "bin"
        } else if has("test") {
            "test"
        } else if has("bench") {
            "bench"
        } else {
            "lib"
        };
        let id = message["package_id"]
            .as_str()
            .context("missing package ID")?;
        let (source, fragment) = id.rsplit_once('#').context("invalid package ID")?;
        let package = fragment.split_once('@').map(|p| p.0).unwrap_or_else(|| {
            source
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or(source)
        });
        let manifest = message["manifest_path"]
            .as_str()
            .context("missing manifest")?;
        let dir = Path::new(manifest)
            .parent()
            .context("missing package directory")?;
        writeln!(
            out,
            "{package}\t{kind}\t{}\t{exe}\t{}",
            message["target"]["name"]
                .as_str()
                .context("missing target name")?,
            dir.display()
        )?;
    }
    Ok(())
}

fn add(warnings: &mut BTreeMap<String, Value>, record: Value, invocation: &str) {
    let key = record.to_string();
    let entry = warnings.entry(key).or_insert_with(|| {
        let mut entry = record;
        entry["invocations"] = json!([]);
        entry["occurrences"] = json!(0);
        entry
    });
    entry["occurrences"] = json!(entry["occurrences"].as_u64().unwrap() + 1);
    let invocations = entry["invocations"].as_array_mut().unwrap();
    if !invocations.iter().any(|v| v == invocation) {
        invocations.push(json!(invocation));
    }
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn collect(dir: &Path) -> Result<BTreeMap<String, Value>> {
    let mut warnings = BTreeMap::new();
    let mut paths = fs::read_dir(dir.join("cargo"))?
        .map(|e| e.map(|e| e.path()))
        .collect::<io::Result<Vec<_>>>()?;
    paths.sort();
    for path in paths {
        let name = path
            .file_name()
            .context("missing filename")?
            .to_string_lossy();
        if let Some(invocation) = name.strip_suffix(".jsonl") {
            for line in BufReader::new(File::open(&path)?).lines() {
                let message: Value = serde_json::from_str(&line?)?;
                if message["reason"] == "compiler-message"
                    && message["message"]["level"] == "warning"
                {
                    let mut diagnostic = message["message"].clone();
                    diagnostic
                        .as_object_mut()
                        .context("invalid diagnostic")?
                        .remove("rendered");
                    add(
                        &mut warnings,
                        json!({"source":"compiler", "package":message["package_id"], "target":message["target"], "diagnostic":diagnostic}),
                        invocation,
                    );
                }
            }
        } else if let Some(invocation) = name.strip_suffix(".stderr.log") {
            let mut block = String::new();
            let flush = |block: &mut String, warnings: &mut BTreeMap<String, Value>| {
                if !block.is_empty() {
                    add(
                        warnings,
                        json!({"source":"cargo-stderr", "message":block.trim_end()}),
                        invocation,
                    );
                    block.clear();
                }
            };
            for line in strip_ansi(&fs::read_to_string(&path)?).lines() {
                if [
                    "Compiling ",
                    "Checking ",
                    "Finished ",
                    "Running ",
                    "Updating ",
                    "Downloading ",
                    "Blocking ",
                    "error:",
                ]
                .iter()
                .any(|prefix| line.trim_start().starts_with(prefix))
                {
                    flush(&mut block, &mut warnings);
                    continue;
                }

                if line.starts_with("warning:") || line.starts_with("warning[") {
                    flush(&mut block, &mut warnings);
                    // Cargo's compiler-warning counts repeat structured diagnostics.
                    if !(line.starts_with("warning: `")
                        && line.contains(" generated ")
                        && line.contains(" warning"))
                    {
                        block.push_str(line);
                        block.push('\n');
                    }
                } else if !block.is_empty()
                    && (line.is_empty()
                        || line.starts_with(char::is_whitespace)
                        || line.starts_with("help:")
                        || line.starts_with("note:"))
                {
                    block.push_str(line);
                    block.push('\n');
                } else {
                    flush(&mut block, &mut warnings);
                }
            }
            flush(&mut block, &mut warnings);
        }
    }
    Ok(warnings)
}

fn summary(dir: &Path) -> Result<()> {
    let warnings = collect(dir)?;
    let mut raw = File::create(dir.join("warnings.jsonl"))?;
    let mut readable = File::create(dir.join("warnings.log"))?;
    for record in warnings.values() {
        writeln!(raw, "{record}")?;
        let invocations = record["invocations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            readable,
            "[{invocations}] ({} occurrences)",
            record["occurrences"]
        )?;
        if record["source"] == "compiler" {
            writeln!(
                readable,
                "{} / {} ({})",
                record["package"].as_str().unwrap_or("?"),
                record["target"]["name"].as_str().unwrap_or("?"),
                record["target"]["kind"]
            )?;
            writeln!(
                readable,
                "{}",
                record["diagnostic"]["message"].as_str().unwrap_or("")
            )?;
            if let Some(spans) = record["diagnostic"]["spans"].as_array() {
                for span in spans.iter().filter(|s| s["is_primary"] == true) {
                    writeln!(
                        readable,
                        "  {}:{}:{}",
                        span["file_name"].as_str().unwrap_or("?"),
                        span["line_start"],
                        span["column_start"]
                    )?;
                }
            }
        } else {
            writeln!(readable, "{}", record["message"].as_str().unwrap_or(""))?;
        }
        writeln!(readable)?;
    }
    println!(
        "Warnings: {} unique; reports: {} and {}",
        warnings.len(),
        dir.join("warnings.log").display(),
        dir.join("warnings.jsonl").display()
    );
    println!(
        "Raw Cargo diagnostics and stderr: {}",
        dir.join("cargo").display()
    );
    Ok(())
}

fn run() -> Result<i32> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("run") if args.len() >= 4 => capture(Path::new(&args[1]), &args[2], &args[3..]),
        Some("index") if args.len() == 3 => {
            index(Path::new(&args[1]), Path::new(&args[2]))?;
            Ok(0)
        }
        Some("summary") if args.len() == 2 => {
            summary(Path::new(&args[1]))?;
            Ok(0)
        }
        _ => bail!(
            "usage: xgpui-test-support run <directory> <suite> <cargo args...> | index <jsonl> <tsv> | summary <directory>"
        ),
    }
}
fn main() {
    match run() {
        Ok(status) => std::process::exit(status),
        Err(error) => {
            eprintln!("test support: {error:#}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn warnings_deduplicate_but_preserve_locations_and_cargo_messages() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let dir = tmp.path();
        fs::create_dir(dir.join("cargo"))?;
        let mut warning = json!({"reason":"compiler-message","package_id":"pkg@1","target":{"name":"pkg","kind":["lib"]},"message":{"level":"warning","message":"unused","spans":[{"is_primary":true,"file_name":"src/lib.rs","line_start":4,"column_start":2}]}});
        for name in ["build", "isolated"] {
            fs::write(
                dir.join(format!("cargo/{name}.jsonl")),
                format!("{warning}\n"),
            )?;
            fs::write(
                dir.join(format!("cargo/{name}.stderr.log")),
                "\x1b[33mwarning: patch unused\x1b[0m\n  explanation\nwarning: `pkg` (lib) generated 1 warning\n    Finished test\n",
            )?;
        }
        warning["message"]["spans"][0]["line_start"] = json!(5);
        writeln!(
            fs::OpenOptions::new()
                .append(true)
                .open(dir.join("cargo/isolated.jsonl"))?,
            "{warning}"
        )?;
        let warnings = collect(dir)?;
        assert_eq!(warnings.len(), 3);
        assert!(
            warnings
                .values()
                .any(|v| v["occurrences"] == 2 && v["invocations"] == json!(["build", "isolated"]))
        );
        summary(dir)?;
        let text = fs::read_to_string(dir.join("warnings.log"))?;
        assert!(text.contains("src/lib.rs:4:2") && text.contains("src/lib.rs:5:2"));
        assert!(text.contains("explanation") && !text.contains("generated 1 warning"));
        Ok(())
    }
    #[test]
    fn indexes_test_artifacts_with_both_package_id_forms() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut messages = String::new();
        for id in [
            "path+file:///repo/pkg#0.1.0",
            "path+file:///repo/renamed#pkg@0.1.0",
        ] {
            let value = json!({"reason":"compiler-artifact","package_id":id,"manifest_path":"/repo/pkg/Cargo.toml","target":{"kind":["bench"],"name":"atlas"},"profile":{"test":false},"executable":"/repo/target/atlas"});
            messages.push_str(&format!("{value}\n"));
        }
        fs::write(tmp.path().join("input"), messages)?;
        index(&tmp.path().join("input"), &tmp.path().join("output"))?;
        assert_eq!(
            fs::read_to_string(tmp.path().join("output"))?,
            "pkg\tbench\tatlas\t/repo/target/atlas\t/repo/pkg\n".repeat(2)
        );
        Ok(())
    }
}
