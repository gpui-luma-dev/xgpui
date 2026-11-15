//! Lab launcher for `apps/xgpui-ext-lab` (#310). Everything it knows comes from
//! `apps/xgpui-ext-lab/labs.toml`; see the comment at the top of that file.
//!
//! `just labs` opens a picker. `just <lab> [view-or-task] [args...]` runs one
//! directly, for example `just media tiled` or `just media capture`.
use anyhow::{Context as _, Result, bail, ensure};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead as _, IsTerminal as _, Write as _},
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

const REGISTRY: &str = "apps/xgpui-ext-lab/labs.toml";
const JUSTFILE: &str = "justfile";
const PLATFORMS: [&str; 4] = ["macos", "linux", "windows", "web"];

const USAGE: &str = "\
just labs                         pick a lab, view or task from a menu
just labs <lab>                   pick one of that lab's views or tasks
just <lab> [view|task] [args...]  run it directly, e.g. `just media tiled`
just labs list [--all]            print every lab, view and task
just labs build | test            build all labs | run the lab tests
just labs check [--help-flags]    labs.toml matches Cargo.toml and the justfile

Options before the lab or command: -n prints the commands without running
them; --profile dev builds a debug binary. Arguments after the view or task
go to the app (`--` before them is optional).";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    defaults: Defaults,
    #[serde(rename = "lab")]
    labs: Vec<Lab>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Defaults {
    manifest: String,
    config: String,
    target_dir: String,
    profile: String,
    features: Vec<String>,
    /// Other packages whose examples must each be run by some task.
    #[serde(default)]
    example_packages: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lab {
    name: String,
    bin: Option<String>,
    example: Option<String>,
    group: String,
    title: String,
    about: String,
    #[serde(default)]
    platforms: Vec<String>,
    #[serde(default)]
    views: Vec<View>,
    #[serde(default, rename = "task")]
    tasks: Vec<Task>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct View {
    name: String,
    title: String,
    args: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Task {
    name: String,
    about: String,
    #[serde(default)]
    platforms: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// Directories to create (relative to the repository root) before the task.
    #[serde(default)]
    mkdir: Vec<String>,
    args: Option<Vec<String>>,
    cargo: Option<Vec<String>>,
    command: Option<Vec<String>>,
    profile: Option<String>,
    #[serde(default)]
    steps: Vec<Step>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Step {
    args: Option<Vec<String>>,
    cargo: Option<Vec<String>>,
    command: Option<Vec<String>>,
    profile: Option<String>,
}

impl Step {
    fn kinds(&self) -> usize {
        [
            self.args.is_some(),
            self.cargo.is_some(),
            self.command.is_some(),
        ]
        .into_iter()
        .filter(|set| *set)
        .count()
    }
}

impl Task {
    /// The top-level step, written inline on the task.
    fn inline(&self) -> Step {
        Step {
            args: self.args.clone(),
            cargo: self.cargo.clone(),
            command: self.command.clone(),
            profile: self.profile.clone(),
        }
    }
    fn steps(&self) -> Vec<Step> {
        if self.steps.is_empty() {
            vec![self.inline()]
        } else {
            self.steps.clone()
        }
    }
    fn available(&self) -> bool {
        on_host(&self.platforms)
    }
}

fn host() -> &'static str {
    std::env::consts::OS
}

fn on_host(platforms: &[String]) -> bool {
    platforms.is_empty() || platforms.iter().any(|p| p == host())
}

/// One thing a lab can do: open it, open a view, or run a task.
enum Item<'a> {
    Open,
    View(&'a View),
    Task(&'a Task),
}

impl Lab {
    /// Whether this lab has a binary to open here (task-only labs have none).
    fn runnable(&self) -> bool {
        (self.bin.is_some() || self.example.is_some()) && on_host(&self.platforms)
    }
    fn target(&self) -> (&'static str, &str) {
        match (&self.bin, &self.example) {
            (Some(bin), _) => ("--bin", bin),
            (_, Some(example)) => ("--example", example),
            _ => unreachable!("only runnable labs are opened"),
        }
    }
    /// What this lab offers on this OS, in menu order.
    fn items(&self) -> Vec<Item<'_>> {
        let mut items = Vec::new();
        if self.runnable() {
            items.push(Item::Open);
            items.extend(self.views.iter().map(Item::View));
        }
        items.extend(self.tasks.iter().filter(|t| t.available()).map(Item::Task));
        items
    }
    fn usable(&self) -> bool {
        !self.items().is_empty()
    }
    fn find(&self, word: &str) -> Option<Item<'_>> {
        if let Some(view) = self.views.iter().find(|v| v.name == word) {
            return Some(Item::View(view));
        }
        self.tasks.iter().find(|t| t.name == word).map(Item::Task)
    }
}

impl Item<'_> {
    fn word(&self) -> Option<&str> {
        match self {
            Item::Open => None,
            Item::View(v) => Some(&v.name),
            Item::Task(t) => Some(&t.name),
        }
    }
    fn describe(&self, lab: &Lab) -> String {
        match self {
            Item::Open => lab.about.clone(),
            Item::View(v) => v.title.clone(),
            Item::Task(t) if t.platforms.is_empty() => t.about.clone(),
            Item::Task(t) => format!("{} [{}]", t.about, t.platforms.join(", ")),
        }
    }
    fn command(&self, lab: &Lab) -> String {
        match self.word() {
            Some(word) => format!("just {} {word}", lab.name),
            None => format!("just {}", lab.name),
        }
    }
}

struct Launcher<'a> {
    root: PathBuf,
    reg: &'a Registry,
    /// Print commands without running them (`just labs -n …`).
    dry_run: bool,
    /// Cargo profile for every command (`just labs --profile dev …`).
    profile: Option<String>,
}

impl Launcher<'_> {
    /// `cargo <sub> --locked --config … --target-dir … --profile …`, plus the lab
    /// package and features when `package` is set.
    fn cargo(&self, sub: &str, profile: Option<&str>, package: bool) -> Vec<String> {
        let d = &self.reg.defaults;
        let mut cmd = vec![
            "cargo".into(),
            sub.into(),
            "--locked".into(),
            "--config".into(),
            d.config.clone(),
            "--target-dir".into(),
            d.target_dir.clone(),
            "--profile".into(),
            self.profile
                .as_deref()
                .or(profile)
                .unwrap_or(&d.profile)
                .into(),
        ];
        if package {
            cmd.extend(["--manifest-path".into(), d.manifest.clone()]);
            cmd.extend(["--features".into(), d.features.join(",")]);
        }
        cmd
    }

    fn run_lab(&self, lab: &Lab, profile: Option<&str>, args: &[String]) -> Vec<String> {
        let (flag, target) = lab.target();
        let mut cmd = self.cargo("run", profile, true);
        cmd.extend([flag.into(), target.into(), "--".into()]);
        cmd.extend(args.iter().cloned());
        cmd
    }

    fn exec(&self, cmd: &[String], env: &BTreeMap<String, String>) -> Result<()> {
        let shown: Vec<String> = env
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .chain(cmd.iter().map(|a| quote(a)))
            .collect();
        eprintln!("$ {}", shown.join(" "));
        if self.dry_run {
            return Ok(());
        }
        let status = Command::new(&cmd[0])
            .args(&cmd[1..])
            .envs(env)
            .current_dir(&self.root)
            .status()
            .with_context(|| format!("could not start {}", cmd[0]))?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
        Ok(())
    }

    fn lab(&self, name: &str) -> Option<&Lab> {
        self.reg.labs.iter().find(|l| l.name == name)
    }

    /// `just <lab> [view|task] [args...]`.
    fn go(&self, lab: &Lab, rest: &[String]) -> Result<()> {
        let (item, rest) = match rest.split_first() {
            Some((word, tail)) if !word.starts_with('-') => match lab.find(word) {
                Some(item) => (item, tail),
                None => {
                    self.print_items(lab);
                    bail!("`{}` has no view or task named `{word}`", lab.name);
                }
            },
            _ => (Item::Open, rest),
        };
        let extra: Vec<String> = match rest.split_first() {
            Some((first, tail)) if first == "--" => tail.to_vec(),
            _ => rest.to_vec(),
        };
        self.run_item(lab, &item, &extra)
    }

    fn run_item(&self, lab: &Lab, item: &Item, extra: &[String]) -> Result<()> {
        match item {
            Item::Open | Item::View(_) => {
                if lab.bin.is_none() && lab.example.is_none() {
                    self.print_items(lab);
                    bail!("`{}` has no app to open; pick one of its tasks", lab.name);
                }
                ensure!(
                    lab.runnable(),
                    "`{}` runs on {} only (this is {})",
                    lab.name,
                    lab.platforms.join(", "),
                    host()
                );
                let mut args = match item {
                    Item::View(view) => view.args.clone(),
                    _ => Vec::new(),
                };
                args.extend(extra.iter().cloned());
                self.exec(&self.run_lab(lab, None, &args), &BTreeMap::new())
            }
            Item::Task(task) => self.run_task(lab, task, extra),
        }
    }

    fn run_task(&self, lab: &Lab, task: &Task, extra: &[String]) -> Result<()> {
        ensure!(
            task.available(),
            "`{} {}` runs on {} only (this is {})",
            lab.name,
            task.name,
            task.platforms.join(", "),
            host()
        );
        for dir in &task.mkdir {
            if self.dry_run {
                eprintln!("$ mkdir -p {dir}");
            } else {
                std::fs::create_dir_all(self.root.join(dir))
                    .with_context(|| format!("creating {dir}"))?;
            }
        }
        let steps = task.steps();
        let placed = steps.iter().any(|s| {
            [&s.args, &s.cargo, &s.command]
                .into_iter()
                .flatten()
                .any(|argv| argv.iter().any(|a| a == "{args}"))
        });
        for (index, step) in steps.iter().enumerate() {
            let last = index + 1 == steps.len();
            let splice = |argv: &[String]| -> Vec<String> {
                let mut out = Vec::new();
                for arg in argv {
                    if arg == "{args}" {
                        out.extend(extra.iter().cloned());
                    } else {
                        out.push(arg.clone());
                    }
                }
                if !placed && last {
                    out.extend(extra.iter().cloned());
                }
                out
            };
            let cmd = if let Some(args) = &step.args {
                self.run_lab(lab, step.profile.as_deref(), &splice(args))
            } else if let Some(cargo) = &step.cargo {
                let (sub, tail) = cargo.split_first().context("empty cargo step")?;
                let package = !tail.iter().any(|a| a == "--manifest-path");
                let mut cmd = self.cargo(sub, step.profile.as_deref(), package);
                cmd.extend(splice(tail));
                cmd
            } else if let Some(command) = &step.command {
                splice(command)
            } else {
                bail!("`{} {}` has an empty step", lab.name, task.name);
            };
            self.exec(&cmd, &task.env)?;
        }
        Ok(())
    }

    /// Labs that have something to offer on this OS, in registry order.
    fn usable_labs(&self) -> Vec<&Lab> {
        self.reg.labs.iter().filter(|l| l.usable()).collect()
    }

    fn print_labs(&self, labs: &[&Lab]) {
        let mut group = "";
        for (index, lab) in labs.iter().enumerate() {
            if lab.group != group {
                group = &lab.group;
                println!("\n{group}");
            }
            println!("  {:>2}  {:<22} {}", index + 1, lab.name, lab.title);
        }
    }

    fn print_items(&self, lab: &Lab) {
        println!("\n{}: {}", lab.title, lab.about);
        for (index, item) in lab.items().iter().enumerate() {
            let cmd = item.command(lab);
            println!("  {:>2}  {cmd:<40} {}", index + 1, item.describe(lab));
        }
    }

    /// `just labs [lab]`: numbered menus on a terminal, a plain list otherwise.
    fn menu(&self, lab: Option<&str>) -> Result<()> {
        let interactive = std::io::stdin().is_terminal();
        let lab = match lab {
            Some(name) => self
                .lab(name)
                .with_context(|| unknown_lab(self.reg, name))?,
            None => {
                let labs = self.usable_labs();
                self.print_labs(&labs);
                if !interactive {
                    println!("\nRun `just labs <lab>` for its views and tasks.");
                    return Ok(());
                }
                let Some(answer) = prompt("\nLab (number or name, Enter to quit): ")? else {
                    return Ok(());
                };
                pick(&labs, &answer, |l| &l.name).with_context(|| format!("no lab `{answer}`"))?
            }
        };
        self.print_items(lab);
        if !interactive {
            return Ok(());
        }
        let items = lab.items();
        ensure!(
            !items.is_empty(),
            "`{}` has nothing to run on {}",
            lab.name,
            host()
        );
        let Some(answer) = prompt("\nRun (number or name, Enter to quit): ")? else {
            return Ok(());
        };
        let index = match answer.parse::<usize>() {
            Ok(n) if (1..=items.len()).contains(&n) => n - 1,
            _ => items
                .iter()
                .position(|i| i.word() == Some(answer.as_str()))
                .with_context(|| format!("`{}` has no `{answer}`", lab.name))?,
        };
        let item = &items[index];
        eprintln!("(next time: {})", item.command(lab));
        self.run_item(lab, item, &[])
    }

    fn list(&self, all: bool) {
        let mut group = "";
        for lab in &self.reg.labs {
            if !all && !lab.usable() {
                continue;
            }
            if lab.group != group {
                group = &lab.group;
                println!("\n{group}");
            }
            let only = if lab.platforms.is_empty() {
                String::new()
            } else {
                format!(" [{}]", lab.platforms.join(", "))
            };
            println!("  {}{only}: {}", lab.title, lab.about);
            let web_only = !lab.platforms.is_empty() && lab.platforms.iter().all(|p| p == "web");
            let has_app = lab.bin.is_some() || lab.example.is_some();
            if (all || lab.runnable()) && has_app && !web_only {
                println!("      just {}", lab.name);
                for view in &lab.views {
                    let cmd = format!("just {} {}", lab.name, view.name);
                    println!("      {cmd:<40} {}", view.title);
                }
            }
            for task in lab.tasks.iter().filter(|t| all || t.available()) {
                let cmd = format!("just {} {}", lab.name, task.name);
                println!("      {cmd:<40} {}", Item::Task(task).describe(lab));
            }
        }
        println!("\nArguments after a lab, view or task go to the app: just media -- --help");
    }

    fn build(&self, args: &[String]) -> Result<()> {
        let mut cmd = self.cargo("build", None, true);
        cmd.push("--bins".into());
        cmd.extend(args.iter().cloned());
        self.exec(&cmd, &BTreeMap::new())
    }

    fn test(&self, args: &[String]) -> Result<()> {
        let mut cmd = self.cargo("test", Some("dev"), true);
        cmd.extend(["--lib", "--bins", "--test", "compatibility"].map(String::from));
        cmd.extend(args.iter().cloned());
        let env = BTreeMap::from([("RUST_BACKTRACE".to_string(), "1".to_string())]);
        self.exec(&cmd, &env)
    }

    fn check(&self, help_flags: bool) -> Result<()> {
        let mut problems = Vec::new();
        let manifest_path = self.root.join(&self.reg.defaults.manifest);
        let manifest: toml::Value = toml::from_str(
            &std::fs::read_to_string(&manifest_path)
                .with_context(|| format!("reading {}", manifest_path.display()))?,
        )?;
        let names = |key: &str| -> BTreeSet<String> {
            manifest
                .get(key)
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .filter_map(|t| t.get("name")?.as_str().map(String::from))
                .collect()
        };
        let (bins, examples) = (names("bin"), names("example"));
        let registered_bins: BTreeSet<String> =
            self.reg.labs.iter().filter_map(|l| l.bin.clone()).collect();
        let registered_examples: BTreeSet<String> = self
            .reg
            .labs
            .iter()
            .filter_map(|l| l.example.clone())
            .collect();
        let manifest_name = &self.reg.defaults.manifest;
        for bin in bins.difference(&registered_bins) {
            problems.push(format!(
                "[[bin]] `{bin}` in {manifest_name} has no lab in {REGISTRY}"
            ));
        }
        for example in examples.difference(&registered_examples) {
            problems.push(format!("[[example]] `{example}` has no lab in {REGISTRY}"));
        }
        for bin in registered_bins.difference(&bins) {
            problems.push(format!(
                "lab bin `{bin}` is not a [[bin]] in {manifest_name}"
            ));
        }
        for example in registered_examples.difference(&examples) {
            problems.push(format!("lab example `{example}` is not an [[example]]"));
        }

        // Real-window probes and other packages' examples must each be run by a task.
        let task_args = |flag: &str, manifest: Option<&str>| -> BTreeSet<String> {
            self.reg
                .labs
                .iter()
                .flat_map(|l| l.tasks.iter())
                .flat_map(|t| t.steps())
                .filter_map(|s| s.cargo)
                .filter(|argv| {
                    let path = argv
                        .windows(2)
                        .find(|w| w[0] == "--manifest-path")
                        .map(|w| w[1].as_str());
                    path == manifest
                })
                .flat_map(|argv| {
                    argv.windows(2)
                        .filter(|w| w[0] == flag)
                        .map(|w| w[1].clone())
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        let window_tests: BTreeSet<String> = manifest
            .get("test")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter(|t| {
                t.get("required-features")
                    .and_then(|f| f.as_array())
                    .is_some_and(|f| {
                        f.iter()
                            .any(|f| f.as_str().is_some_and(|f| f.ends_with("-window-tests")))
                    })
            })
            .filter_map(|t| t.get("name")?.as_str().map(String::from))
            .collect();
        for test in window_tests.difference(&task_args("--test", None)) {
            problems.push(format!(
                "window test `{test}` in {manifest_name} has no task in {REGISTRY}"
            ));
        }
        for package in &self.reg.defaults.example_packages {
            let package_manifest = format!("{package}/Cargo.toml");
            let mut examples = BTreeSet::new();
            if let Ok(text) = std::fs::read_to_string(self.root.join(&package_manifest)) {
                let value: toml::Value = toml::from_str(&text)?;
                examples.extend(
                    value
                        .get("example")
                        .and_then(|v| v.as_array())
                        .into_iter()
                        .flatten()
                        .filter_map(|t| t.get("name")?.as_str().map(String::from)),
                );
            } else {
                problems.push(format!("example package `{package}` has no Cargo.toml"));
            }
            if let Ok(entries) = std::fs::read_dir(self.root.join(package).join("examples")) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let is_example = path.extension().is_some_and(|e| e == "rs")
                        || path.join("main.rs").is_file();
                    if is_example && let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        examples.insert(stem.to_string());
                    }
                }
            }
            for example in examples.difference(&task_args("--example", Some(&package_manifest))) {
                problems.push(format!(
                    "example `{example}` in {package} has no task in {REGISTRY}"
                ));
            }
        }

        // Every lab needs its own `just <lab>` entry point.
        let justfile = std::fs::read_to_string(self.root.join(JUSTFILE))
            .with_context(|| format!("reading {JUSTFILE}"))?;
        let recipes = just_names(&justfile);
        let reserved = ["labs", "list", "build", "test", "check", "help"];

        let mut lab_names = BTreeSet::new();
        for lab in &self.reg.labs {
            let at = format!("lab `{}`", lab.name);
            if !lab_names.insert(&lab.name) {
                problems.push(format!("{at} is registered twice"));
            }
            if reserved.contains(&lab.name.as_str()) {
                problems.push(format!("{at}: `{}` is a launcher command", lab.name));
            }
            if !recipes.contains(lab.name.as_str()) {
                problems.push(format!(
                    "{at} has no `{}` recipe in {JUSTFILE} (add `{} *args:` calling the launcher)",
                    lab.name, lab.name
                ));
            }
            if lab.bin.is_some() && lab.example.is_some() {
                problems.push(format!("{at} has both `bin` and `example`"));
            }
            if lab.bin.is_none() && lab.example.is_none() && lab.tasks.is_empty() {
                problems.push(format!("{at} has no `bin`, `example` or tasks"));
            }
            for p in &lab.platforms {
                if !PLATFORMS.contains(&p.as_str()) {
                    problems.push(format!("{at}: unknown platform `{p}`"));
                }
            }
            // Views and tasks share one namespace: `just <lab> <word>`.
            let mut words = BTreeSet::new();
            for word in lab
                .views
                .iter()
                .map(|v| &v.name)
                .chain(lab.tasks.iter().map(|t| &t.name))
            {
                if !words.insert(word) {
                    problems.push(format!("{at}: `{word}` names more than one view or task"));
                }
                if word.starts_with('-') {
                    problems.push(format!("{at}: `{word}` must not start with `-`"));
                }
            }
            for task in &lab.tasks {
                let at = format!("{at} task `{}`", task.name);
                for p in &task.platforms {
                    if !PLATFORMS.contains(&p.as_str()) {
                        problems.push(format!("{at}: unknown platform `{p}`"));
                    }
                }
                if !task.steps.is_empty() && task.inline().kinds() > 0 {
                    problems.push(format!("{at}: use either `steps` or one top-level step"));
                }
                for step in task.steps() {
                    if step.kinds() != 1 {
                        problems.push(format!(
                            "{at}: each step needs exactly one of args, cargo, command"
                        ));
                    }
                    let files = step
                        .cargo
                        .iter()
                        .flat_map(|argv| argv.windows(2))
                        .filter(|w| w[0] == "--manifest-path")
                        .map(|w| w[1].clone())
                        .chain(
                            step.command
                                .iter()
                                .flatten()
                                .filter(|a| a.ends_with(".sh") || a.ends_with(".py"))
                                .cloned(),
                        );
                    for file in files {
                        if !self.root.join(&file).is_file() {
                            problems.push(format!("{at}: `{file}` does not exist"));
                        }
                    }
                }
            }
        }

        if help_flags {
            for lab in self
                .reg
                .labs
                .iter()
                .filter(|l| l.runnable() && !l.views.is_empty())
            {
                let (flag, target) = lab.target();
                let mut cmd = self.cargo("run", None, true);
                cmd.extend([
                    flag.into(),
                    target.into(),
                    "--quiet".into(),
                    "--".into(),
                    "--help".into(),
                ]);
                eprintln!("checking {} --help", lab.name);
                let out = Command::new(&cmd[0])
                    .args(&cmd[1..])
                    .current_dir(&self.root)
                    .output()?;
                let help = String::from_utf8_lossy(&out.stdout);
                for view in &lab.views {
                    for arg in view.args.iter().filter(|a| a.starts_with("--")) {
                        if !help.contains(arg.as_str()) {
                            problems.push(format!(
                                "lab `{}` view `{}`: `{arg}` is not in `{target} --help`",
                                lab.name, view.name
                            ));
                        }
                    }
                }
            }
        }

        if problems.is_empty() {
            println!(
                "{REGISTRY}: {} labs, {} views, {} tasks; matches {manifest_name} and {JUSTFILE}",
                self.reg.labs.len(),
                self.reg.labs.iter().map(|l| l.views.len()).sum::<usize>(),
                self.reg.labs.iter().map(|l| l.tasks.len()).sum::<usize>(),
            );
            Ok(())
        } else {
            for problem in &problems {
                eprintln!("error: {problem}");
            }
            bail!("{} problem(s) in {REGISTRY}", problems.len())
        }
    }
}

/// Recipe and alias names defined in a justfile.
fn just_names(justfile: &str) -> BTreeSet<&str> {
    justfile
        .lines()
        .filter_map(|line| {
            if let Some(rest) = line.strip_prefix("alias ") {
                return rest.split_whitespace().next();
            }
            let first = line.chars().next()?;
            if !first.is_ascii_lowercase() {
                return None;
            }
            let name = line.split([' ', ':']).next()?;
            let after = line[name.len()..].trim_start();
            (after.starts_with(':') && !after.starts_with(":=") || after.starts_with('*'))
                .then_some(name)
        })
        .collect()
}

fn unknown_lab(reg: &Registry, name: &str) -> String {
    let names: Vec<_> = reg.labs.iter().map(|l| l.name.as_str()).collect();
    format!("no lab named `{name}`; labs: {}", names.join(", "))
}

/// Pick by 1-based number or exact name.
fn pick<'a, T>(items: &[&'a T], answer: &str, name: impl Fn(&T) -> &str) -> Option<&'a T> {
    match answer.parse::<usize>() {
        Ok(n) if (1..=items.len()).contains(&n) => Some(items[n - 1]),
        _ => items.iter().copied().find(|item| name(item) == answer),
    }
}

/// Read one trimmed line; `None` on Enter or end of input.
fn prompt(text: &str) -> Result<Option<String>> {
    print!("{text}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    let line = line.trim();
    Ok((!line.is_empty()).then(|| line.to_string()))
}

fn quote(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@".contains(c))
    {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

/// The repository root: the nearest ancestor of the current directory that
/// holds the registry, falling back to this crate's location.
fn root() -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    for dir in cwd.ancestors() {
        if dir.join(REGISTRY).is_file() {
            return Ok(dir.to_path_buf());
        }
    }
    let fallback = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    ensure!(
        fallback.join(REGISTRY).is_file(),
        "could not find {REGISTRY}"
    );
    Ok(fallback)
}

fn main() -> ExitCode {
    match real_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn real_main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let (mut dry_run, mut profile) = (false, None);
    loop {
        match args.first().map(String::as_str) {
            Some("-n" | "--dry-run") => {
                dry_run = true;
                args.remove(0);
            }
            Some("--profile") => {
                ensure!(args.len() > 1, "--profile needs a value");
                profile = Some(args.remove(1));
                args.remove(0);
            }
            _ => break,
        }
    }
    let root = root()?;
    let text = std::fs::read_to_string(root.join(REGISTRY))
        .with_context(|| format!("reading {REGISTRY}"))?;
    let reg: Registry = toml::from_str(&text).with_context(|| format!("parsing {REGISTRY}"))?;
    let launcher = Launcher {
        root,
        reg: &reg,
        dry_run,
        profile,
    };
    let Some((first, rest)) = args.split_first() else {
        return launcher.menu(None);
    };
    match first.as_str() {
        "list" => {
            launcher.list(rest.iter().any(|a| a == "--all"));
            Ok(())
        }
        "build" => launcher.build(rest),
        "test" => launcher.test(rest),
        "check" => launcher.check(rest.iter().any(|a| a == "--help-flags")),
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        // `just labs media`: that lab's menu.
        name if rest.is_empty() && launcher.lab(name).is_some() && !direct() => {
            launcher.menu(Some(name))
        }
        name => match launcher.lab(name) {
            Some(lab) => launcher.go(lab, rest),
            None => bail!("{}\n\n{USAGE}", unknown_lab(&reg, name)),
        },
    }
}

/// `just <lab>` sets this so a bare lab name opens the lab instead of its menu.
fn direct() -> bool {
    std::env::var_os("XGPUI_LAB_DIRECT").is_some()
}
