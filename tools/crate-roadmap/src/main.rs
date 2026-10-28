//! Source-based inventory. Deliberately does not claim rustc export resolution.

use anyhow::{Context, Result, bail};
use quote::ToTokens;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use syn::{Attribute, Item, Visibility, spanned::Spanned};

#[derive(Serialize)]
struct Declaration {
    name: String,
    kind: String,
    module: String,
    signature: String,
    file: String,
    line: usize,
    guards: Vec<String>,
    module_public: bool,
    root_export: bool,
}

#[derive(Serialize)]
struct Dependency {
    package: String,
    alias: String,
    kind: String,
    target: String,
    optional: bool,
}

#[derive(Serialize)]
struct Crate {
    package: String,
    published_package: String,
    version: String,
    description: String,
    entry: String,
    procedural: bool,
    features: toml::Value,
    dependencies: Vec<Dependency>,
    declarations: Vec<Declaration>,
    files: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Default)]
struct Scan {
    declarations: Vec<Declaration>,
    files: BTreeMap<String, String>,
    visited: BTreeSet<(PathBuf, String)>,
    warnings: Vec<String>,
}

fn tokens(value: &impl ToTokens) -> String {
    value.to_token_stream().to_string()
}

fn is_public(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

fn test_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<syn::Path>()
                .is_ok_and(|p| p.is_ident("test"))
    })
}

fn guards(attrs: &[Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter(|a| {
            a.path().is_ident("cfg")
                || a.path().is_ident("cfg_attr")
                || (a.path().is_ident("doc") && tokens(&a.meta).contains("hidden"))
        })
        .map(|a| tokens(&a.meta))
        .collect()
}

impl Scan {
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        name: impl Into<String>,
        kind: &str,
        signature: String,
        module: &str,
        file: &str,
        line: usize,
        attrs: &[Attribute],
        inherited: &[String],
        module_public: bool,
        root_export: bool,
    ) {
        if test_only(attrs) {
            return;
        }
        let mut conditions = inherited.to_vec();
        conditions.extend(guards(attrs));
        self.declarations.push(Declaration {
            name: name.into(),
            kind: kind.into(),
            signature,
            module: module.into(),
            file: file.into(),
            line,
            guards: conditions,
            module_public,
            root_export,
        });
    }

    fn file(
        &mut self,
        root: &Path,
        path: &Path,
        module: &str,
        directory: &Path,
        inherited: &[String],
        public: bool,
    ) -> Result<()> {
        if !self.visited.insert((path.to_path_buf(), module.into())) {
            return Ok(());
        }
        let source =
            fs::read_to_string(path).with_context(|| format!("Reading {}", path.display()))?;
        let syntax =
            syn::parse_file(&source).with_context(|| format!("Parsing {}", path.display()))?;
        let file = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        self.files.insert(file.clone(), source);
        let mut conditions = inherited.to_vec();
        conditions.extend(guards(&syntax.attrs));
        self.items(
            root,
            &syntax.items,
            module,
            &file,
            directory,
            path.parent().context("Module has no parent")?,
            &conditions,
            public,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn items(
        &mut self,
        root: &Path,
        items: &[Item],
        module: &str,
        file: &str,
        directory: &Path,
        path_directory: &Path,
        inherited: &[String],
        public: bool,
    ) -> Result<()> {
        for item in items {
            let attrs = match item {
                Item::Struct(i) => &i.attrs,
                Item::Enum(i) => &i.attrs,
                Item::Trait(i) => &i.attrs,
                Item::Type(i) => &i.attrs,
                Item::Fn(i) => &i.attrs,
                Item::Mod(i) => &i.attrs,
                Item::Use(i) => &i.attrs,
                Item::Impl(i) => &i.attrs,
                Item::Const(i) => &i.attrs,
                Item::Static(i) => &i.attrs,
                Item::Macro(i) => &i.attrs,
                Item::Union(i) => &i.attrs,
                _ => continue,
            };
            if test_only(attrs) {
                continue;
            }
            match item {
                Item::Mod(i) => {
                    if is_public(&i.vis) {
                        self.add(
                            i.ident.to_string(),
                            "module",
                            format!("pub mod {}", i.ident),
                            module,
                            file,
                            i.ident.span().start().line,
                            attrs,
                            inherited,
                            public,
                            module == "crate",
                        );
                    }
                    let child = format!("{module}::{}", i.ident);
                    let mut conditions = inherited.to_vec();
                    conditions.extend(guards(attrs));
                    let child_public = public && is_public(&i.vis);
                    let explicit_path = attrs.iter().find_map(|a| {
                        if a.path().is_ident("path") {
                            if let syn::Meta::NameValue(v) = &a.meta
                                && let syn::Expr::Lit(l) = &v.value
                                && let syn::Lit::Str(s) = &l.lit
                            {
                                return Some(s.value());
                            }
                            None
                        } else {
                            None
                        }
                    });
                    if let Some((_, children)) = &i.content {
                        let inline_directory = directory
                            .join(explicit_path.as_deref().unwrap_or(&i.ident.to_string()));
                        self.items(
                            root,
                            children,
                            &child,
                            file,
                            &inline_directory,
                            &inline_directory,
                            &conditions,
                            child_public,
                        )?;
                    } else {
                        let flat = directory.join(format!("{}.rs", i.ident));
                        let nested = directory.join(i.ident.to_string()).join("mod.rs");
                        let path = explicit_path
                            .map(|p| path_directory.join(p))
                            .unwrap_or_else(|| if flat.is_file() { flat } else { nested });
                        if path.is_file() {
                            let next = if path.file_name().is_some_and(|n| n == "mod.rs") {
                                path.parent().context("Module has no parent")?.to_path_buf()
                            } else {
                                path.parent()
                                    .context("Module has no parent")?
                                    .join(path.file_stem().context("Missing stem")?)
                            };
                            self.file(root, &path, &child, &next, &conditions, child_public)?;
                        } else {
                            self.warnings.push(format!(
                                "Unresolved module {child}: {}",
                                path.strip_prefix(root)?.display()
                            ));
                        }
                    }
                }
                Item::Use(i) if is_public(&i.vis) => self.add(
                    tokens(&i.tree),
                    "re-export",
                    tokens(i),
                    module,
                    file,
                    i.use_token.span.start().line,
                    attrs,
                    inherited,
                    public,
                    module == "crate",
                ),
                Item::Fn(i) if is_public(&i.vis) => {
                    let procedure = attrs.iter().find(|a| {
                        a.path().is_ident("proc_macro_derive")
                            || a.path().is_ident("proc_macro_attribute")
                            || a.path().is_ident("proc_macro")
                    });
                    let (kind, signature) = if let Some(a) = procedure {
                        (
                            "procedural macro",
                            format!("#[{}] {}", tokens(&a.meta), i.sig.ident),
                        )
                    } else {
                        ("function", format!("pub {}", tokens(&i.sig)))
                    };
                    let name = procedure.filter(|a| a.path().is_ident("proc_macro_derive"))
                        .and_then(|a| a.parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated).ok())
                        .and_then(|args| args.first().map(tokens)).unwrap_or_else(|| i.sig.ident.to_string());
                    self.add(
                        name,
                        kind,
                        signature,
                        module,
                        file,
                        i.sig.ident.span().start().line,
                        attrs,
                        inherited,
                        public,
                        module == "crate",
                    );
                }
                Item::Struct(i) if is_public(&i.vis) => {
                    self.add(
                        i.ident.to_string(),
                        "struct",
                        format!(
                            "pub struct {}{} {}",
                            i.ident,
                            tokens(&i.generics),
                            tokens(&i.generics.where_clause)
                        ),
                        module,
                        file,
                        i.ident.span().start().line,
                        attrs,
                        inherited,
                        public,
                        module == "crate",
                    );
                    for (index, field) in i
                        .fields
                        .iter()
                        .enumerate()
                        .filter(|(_, f)| is_public(&f.vis))
                    {
                        let name = field
                            .ident
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| index.to_string());
                        let mut conditions = inherited.to_vec();
                        conditions.extend(guards(attrs));
                        self.add(
                            format!("{}::{name}", i.ident),
                            "field",
                            format!("pub {name}: {}", tokens(&field.ty)),
                            module,
                            file,
                            field.span().start().line,
                            &field.attrs,
                            &conditions,
                            public,
                            false,
                        );
                    }
                }
                Item::Enum(i) if is_public(&i.vis) => {
                    self.add(
                        i.ident.to_string(),
                        "enum",
                        format!("pub enum {}{}", i.ident, tokens(&i.generics)),
                        module,
                        file,
                        i.ident.span().start().line,
                        attrs,
                        inherited,
                        public,
                        module == "crate",
                    );
                    for variant in &i.variants {
                        let mut conditions = inherited.to_vec();
                        conditions.extend(guards(attrs));
                        self.add(
                            format!("{}::{}", i.ident, variant.ident),
                            "variant",
                            tokens(variant),
                            module,
                            file,
                            variant.ident.span().start().line,
                            &variant.attrs,
                            &conditions,
                            public,
                            false,
                        );
                    }
                }
                Item::Trait(i) if is_public(&i.vis) => {
                    self.add(
                        i.ident.to_string(),
                        "trait",
                        format!(
                            "pub trait {}{}: {}",
                            i.ident,
                            tokens(&i.generics),
                            tokens(&i.supertraits)
                        ),
                        module,
                        file,
                        i.ident.span().start().line,
                        attrs,
                        inherited,
                        public,
                        module == "crate",
                    );
                    let mut conditions = inherited.to_vec();
                    conditions.extend(guards(attrs));
                    for member in &i.items {
                        match member {
                            syn::TraitItem::Fn(f) => self.add(
                                format!("{}::{}", i.ident, f.sig.ident),
                                "trait method",
                                tokens(&f.sig),
                                module,
                                file,
                                f.sig.ident.span().start().line,
                                &f.attrs,
                                &conditions,
                                public,
                                false,
                            ),
                            syn::TraitItem::Type(t) => self.add(
                                format!("{}::{}", i.ident, t.ident),
                                "associated type",
                                tokens(t),
                                module,
                                file,
                                t.ident.span().start().line,
                                &t.attrs,
                                &conditions,
                                public,
                                false,
                            ),
                            syn::TraitItem::Const(c) => self.add(
                                format!("{}::{}", i.ident, c.ident),
                                "associated const",
                                tokens(c),
                                module,
                                file,
                                c.ident.span().start().line,
                                &c.attrs,
                                &conditions,
                                public,
                                false,
                            ),
                            _ => (),
                        }
                    }
                }
                Item::Impl(i) => {
                    let owner = tokens(&i.self_ty);
                    let mut conditions = inherited.to_vec();
                    conditions.extend(guards(attrs));
                    if let Some((_, tr, _)) = &i.trait_ {
                        self.add(
                            format!("{} for {owner}", tokens(tr)),
                            "trait implementation",
                            format!(
                                "impl{} {} for {owner} {}",
                                tokens(&i.generics),
                                tokens(tr),
                                tokens(&i.generics.where_clause)
                            ),
                            module,
                            file,
                            i.impl_token.span.start().line,
                            attrs,
                            inherited,
                            public,
                            false,
                        );
                    } else {
                        for member in &i.items {
                            match member {
                                syn::ImplItem::Fn(f)
                                    if is_public(&f.vis) && !test_only(&f.attrs) =>
                                {
                                    self.add(
                                        format!("{owner}::{}", f.sig.ident),
                                        "method",
                                        format!("pub {}", tokens(&f.sig)),
                                        module,
                                        file,
                                        f.sig.ident.span().start().line,
                                        &f.attrs,
                                        &conditions,
                                        public,
                                        false,
                                    )
                                }
                                syn::ImplItem::Const(c) if is_public(&c.vis) => self.add(
                                    format!("{owner}::{}", c.ident),
                                    "associated const",
                                    tokens(c),
                                    module,
                                    file,
                                    c.ident.span().start().line,
                                    &c.attrs,
                                    &conditions,
                                    public,
                                    false,
                                ),
                                _ => (),
                            }
                        }
                    }
                }
                Item::Type(i) if is_public(&i.vis) => self.add(
                    i.ident.to_string(),
                    "type alias",
                    tokens(i),
                    module,
                    file,
                    i.ident.span().start().line,
                    attrs,
                    inherited,
                    public,
                    module == "crate",
                ),
                Item::Const(i) if is_public(&i.vis) => self.add(
                    i.ident.to_string(),
                    "constant",
                    format!("pub const {}: {}", i.ident, tokens(&i.ty)),
                    module,
                    file,
                    i.ident.span().start().line,
                    attrs,
                    inherited,
                    public,
                    module == "crate",
                ),
                Item::Static(i) if is_public(&i.vis) => self.add(
                    i.ident.to_string(),
                    "static",
                    format!("pub static {}: {}", i.ident, tokens(&i.ty)),
                    module,
                    file,
                    i.ident.span().start().line,
                    attrs,
                    inherited,
                    public,
                    module == "crate",
                ),
                Item::Union(i) if is_public(&i.vis) => self.add(
                    i.ident.to_string(),
                    "union",
                    format!("pub union {}{}", i.ident, tokens(&i.generics)),
                    module,
                    file,
                    i.ident.span().start().line,
                    attrs,
                    inherited,
                    public,
                    module == "crate",
                ),
                Item::Macro(i) if attrs.iter().any(|a| a.path().is_ident("macro_export")) => {
                    let name = i
                        .ident
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| tokens(&i.mac.path));
                    self.add(
                        &name,
                        "exported macro",
                        format!("macro_rules! {name}"),
                        module,
                        file,
                        i.span().start().line,
                        attrs,
                        inherited,
                        public,
                        true,
                    );
                }
                Item::Macro(i) if i.mac.path.is_ident("include") => self.warnings.push(format!(
                    "Unexpanded include! at {file}:{}",
                    i.span().start().line
                )),
                _ => (),
            }
        }
        Ok(())
    }
}

fn dependencies(
    table: &toml::Value,
    target: &str,
    packages: &BTreeMap<String, String>,
    output: &mut Vec<Dependency>,
) {
    for (key, kind) in [
        ("dependencies", "normal"),
        ("dev-dependencies", "dev"),
        ("build-dependencies", "build"),
    ] {
        if let Some(entries) = table.get(key).and_then(toml::Value::as_table) {
            for (alias, value) in entries {
                let package = value
                    .get("package")
                    .and_then(toml::Value::as_str)
                    .unwrap_or(alias);
                if let Some(package) = packages.get(package) {
                    output.push(Dependency {
                        package: package.into(),
                        alias: alias.into(),
                        kind: kind.into(),
                        target: target.into(),
                        optional: value
                            .get("optional")
                            .and_then(toml::Value::as_bool)
                            .unwrap_or(false),
                    });
                }
            }
        }
    }
}

fn html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn generate(root: &Path) -> Result<()> {
    let provenance: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("upstream.json"))?)?;
    let extension_packages = BTreeMap::from([
        ("xgpui-color", "extensions/xgpui-ext/crates/xgpui-color"),
        ("xgpui-shader", "extensions/xgpui-ext/crates/xgpui-shader"),
        ("xgpui-gamut", "extensions/xgpui-ext/crates/xgpui-gamut"),
        ("xgpui-parley", "extensions/xgpui-ext/crates/xgpui-parley"),
        ("xgpui-media", "extensions/xgpui-ext/crates/xgpui-media"),
        ("xgpui-svg", "extensions/xgpui-ext/crates/xgpui-svg"),
    ]);
    let mut packages = provenance["packages"]
        .as_object()
        .context("Missing captured package map")?
        .clone();
    // Local extensions are indexed like crates without claiming upstream provenance.
    for (package, directory) in &extension_packages {
        packages.insert(
            (*package).into(),
            serde_json::Value::String((*directory).into()),
        );
    }
    let mut names: BTreeMap<String, String> = packages
        .keys()
        .map(|name| (name.clone(), name.clone()))
        .collect();
    if let Some(identities) = provenance
        .get("distribution")
        .and_then(|d| d.get("packages"))
        .and_then(serde_json::Value::as_object)
    {
        for (local, published) in identities {
            names.insert(
                published
                    .as_str()
                    .context("Invalid package identity")?
                    .into(),
                local.clone(),
            );
        }
    }
    let output = root.join("docs");
    fs::create_dir_all(&output)?;
    // This directory contains only generated pages. Rebuild it so removed crates
    // and modules cannot remain browsable after regenerating an existing site.
    let source_output = output.join("source");
    if source_output.exists() {
        fs::remove_dir_all(&source_output)?;
    }
    let mut crates = Vec::new();
    for (package, directory) in &packages {
        let directory = root.join(directory.as_str().context("Invalid package path")?);
        let manifest: toml::Value =
            toml::from_str(&fs::read_to_string(directory.join("Cargo.toml"))?)?;
        let entry = manifest
            .get("lib")
            .and_then(|v| v.get("path"))
            .and_then(toml::Value::as_str)
            .unwrap_or("src/lib.rs");
        let path = directory.join(entry);
        let mut scan = Scan::default();
        scan.file(
            root,
            &path,
            "crate",
            path.parent().context("Missing library directory")?,
            &[],
            true,
        )?;
        let mut deps = Vec::new();
        dependencies(&manifest, "", &names, &mut deps);
        if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
            for (target, table) in targets {
                dependencies(table, target, &names, &mut deps);
            }
        }
        for (file, source) in &scan.files {
            let target = output.join("source").join(format!("{file}.html"));
            fs::create_dir_all(target.parent().context("Missing source output directory")?)?;
            let lines: String = source
                .lines()
                .enumerate()
                .map(|(i, text)| {
                    format!(
                        "<span class=\"line\" id=\"L{}\"><a href=\"#L{}\">{}</a>{}</span>\n",
                        i + 1,
                        i + 1,
                        i + 1,
                        html(text)
                    )
                })
                .collect();
            let page = format!(
                "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{0} · source</title><style>body{{background:#111111;color:#dddddd;font:14px/1.65 ui-monospace,monospace;margin:24px}}h1{{font:16px system-ui}}pre{{tab-size:4}}.line{{display:block;white-space:pre}}.line a{{display:inline-block;width:5em;color:#999999;text-decoration:none;user-select:none}}.line:target{{background:#333333;color:white}}a{{color:#cccccc}}</style><h1>{0}</h1><p>Source snapshot, Apache-2.0. Line anchors are one-based. <a href=\"javascript:history.back()\">Back to roadmap</a></p><div>{1}</div></html>",
                html(file),
                lines
            );
            fs::write(target, page)?;
        }
        eprintln!(
            "{package}: {} declarations, {} source files, {} unresolved items",
            scan.declarations.len(),
            scan.files.len(),
            scan.warnings.len()
        );
        crates.push(Crate {
            package: package.clone(),
            published_package: manifest["package"]["name"]
                .as_str()
                .unwrap_or(package)
                .into(),
            version: manifest["package"]["version"].as_str().unwrap_or("").into(),
            description: manifest["package"]
                .get("description")
                .and_then(toml::Value::as_str)
                .unwrap_or("")
                .into(),
            entry: path.strip_prefix(root)?.to_string_lossy().into(),
            procedural: manifest
                .get("lib")
                .and_then(|v| v.get("proc-macro"))
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
            features: manifest
                .get("features")
                .cloned()
                .unwrap_or_else(|| toml::Value::Table(Default::default())),
            dependencies: deps,
            files: scan.files.keys().cloned().collect(),
            declarations: scan.declarations,
            warnings: scan.warnings,
        });
    }
    let notes: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("docs/notes.json"))?)?;
    if notes.as_object().map_or(0, |n| n.len()) != crates.len() {
        bail!("Notes must cover exactly the captured crates");
    }
    for krate in &crates {
        if notes.get(&krate.package).is_none() {
            bail!("Missing notes for {}", krate.package);
        }
    }
    // Published guides live in the web documentation source tree.
    // Embed them so navigation works offline without fetch.
    let document_sources = [
        ("local-development", "docs/LOCAL_DEVELOPMENT.md"),
        ("extension-lab", "docs/extensions/extension-lab.md"),
        ("color-math", "docs/extensions/color-math.md"),
        ("media-images", "docs/extensions/media-images.md"),
        ("media-pipeline", "docs/extensions/media-pipeline.md"),
        ("svg-rendering", "docs/extensions/svg-rendering.md"),
        ("parley-text", "docs/extensions/parley-text.md"),
        (
            "high-gamut-windows",
            "docs/extensions/high-gamut-windows.md",
        ),
        ("shader-producers", "docs/extensions/shader-producers.md"),
        ("gpui-public-api", "docs/extensions/gpui-public-api.md"),
        ("extension-paints", "docs/extensions/extension-paints.md"),
        (
            "paint-contract-hardening",
            "docs/extensions/paint-contract-hardening.md",
        ),
        ("portable-paints", "docs/extensions/portable-paints.md"),
        (
            "native-dx11-paints",
            "docs/extensions/native-dx11-paints.md",
        ),
        ("portable-wgsl-v1", "docs/extensions/portable-wgsl-v1.md"),
        ("instanced-wgsl-v2", "docs/extensions/instanced-wgsl-v2.md"),
        ("brushes", "docs/extensions/brushes.md"),
        ("typography", "docs/extensions/typography.md"),
        ("paint-validation", "docs/extensions/paint-validation.md"),
    ];
    let mut extension_documents = BTreeMap::new();
    for (route, file) in document_sources {
        let markdown = fs::read_to_string(root.join(file))?;
        let title = markdown
            .lines()
            .next()
            .unwrap_or(route)
            .trim_start_matches("# ");
        let markdown = markdown
            .lines()
            .map(|line| {
                if line.starts_with('#')
                    && let Some((_, heading)) = line.split_once(' ')
                {
                    let slug: String = heading
                        .to_lowercase()
                        .chars()
                        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-')
                        .map(|c| if c == ' ' { '-' } else { c })
                        .collect();
                    format!("{line} {{#{slug}}}")
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let parser = pulldown_cmark::Parser::new_ext(
            &markdown,
            pulldown_cmark::Options::ENABLE_TABLES
                | pulldown_cmark::Options::ENABLE_HEADING_ATTRIBUTES,
        )
        .map(|event| match event {
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let filename = dest_url
                    .split('#')
                    .next()
                    .unwrap_or("")
                    .rsplit('/')
                    .next()
                    .unwrap_or("");
                let replacement = document_sources
                    .iter()
                    .find(|(_, source)| source.rsplit('/').next() == Some(filename));
                let dest_url = replacement
                    .map(|(route, _)| {
                        format!(
                            "#{route}{}",
                            dest_url
                                .split_once('#')
                                .map(|(_, section)| format!("&section={section}"))
                                .unwrap_or_default()
                        )
                        .into()
                    })
                    .unwrap_or_else(|| {
                        // Static report assets are relative to the guide in Markdown,
                        // but embedded guide HTML is served from the docs root.
                        if let Some(report) = dest_url.strip_prefix("../benchmarks/") {
                            format!("benchmarks/{report}").into()
                        } else if let Some(example) = dest_url.strip_prefix("../examples/") {
                            format!("examples/{example}").into()
                        } else {
                            dest_url
                        }
                    });
                pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    id,
                })
            }
            other => other,
        });
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, parser);
        // Glossary references leave the guide open. Arrows live in Markdown so
        // they also appear when reading source documentation on GitHub.
        html = html.replace(
            "<a href=\"https://en.wikipedia.org/wiki/",
            "<a target=\"_blank\" rel=\"noopener noreferrer\" href=\"https://en.wikipedia.org/wiki/",
        );
        extension_documents.insert(
            route,
            serde_json::json!({"title": title, "source": file, "html": html}),
        );
    }
    let data = serde_json::json!({"extension_packages": extension_packages, "extension_documents": extension_documents, "provenance": provenance, "crates": crates, "notes": notes,
        "methodology": "Syntactic Rust inventory, not rustc's resolved public API. pub(crate) excluded; plain cfg(test) modules excluded. Other cfg conditions retained without evaluation. Glob re-exports, associated-type reachability, macro expansions and external re-exports are not resolved. Private-module declarations and trait implementations are candidates, not proof of consumer access. Signatures are source heads; follow source for full bounds and context."});
    let json = serde_json::to_string(&data)?
        .replace('<', "\\u003c")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    fs::write(
        output.join("data.js"),
        format!("window.ROADMAP = {json};\n"),
    )?;
    Ok(())
}

fn main() -> Result<()> {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    generate(&root.canonicalize()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visibility_cfg_and_reexports_are_distinguished() {
        let syntax = syn::parse_file("pub struct Visible { pub field: u8, hidden: u8 } pub(crate) struct Internal; pub use hidden::Exported; mod hidden { pub struct Exported; } #[cfg(test)] mod tests { pub struct NotApi; } #[cfg(feature = \"test-support\")] pub fn helper() {} ").expect("fixture parses");
        let mut scan = Scan::default();
        scan.items(
            Path::new("."),
            &syntax.items,
            "crate",
            "fixture.rs",
            Path::new("."),
            Path::new("."),
            &[],
            true,
        )
        .expect("fixture scans");
        assert!(
            !scan
                .declarations
                .iter()
                .any(|i| i.name == "Internal" || i.name == "NotApi" || i.name == "Visible::hidden")
        );
        assert!(
            scan.declarations
                .iter()
                .any(|i| i.name == "Exported" && !i.module_public)
        );
        assert!(
            scan.declarations
                .iter()
                .any(|i| i.kind == "re-export" && i.root_export)
        );
        assert!(
            scan.declarations
                .iter()
                .find(|i| i.name == "helper")
                .is_some_and(|i| !i.guards.is_empty())
        );
    }

    #[test]
    fn explicit_module_paths_and_diagnostics_are_relative() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "xgpui-module-paths-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(root.join("src/outer/inline"))?;
        fs::create_dir_all(root.join("src/nested"))?;
        fs::write(root.join("src/lib.rs"), "mod outer; mod nested;")?;
        fs::write(
            root.join("src/outer.rs"),
            r##"#[path = "sibling.rs"] mod explicit;
mod inline { #[path = "leaf.rs"] mod leaf; }
#[path = "missing.rs"] mod missing;
include!("generated.rs");"##,
        )?;
        fs::write(root.join("src/sibling.rs"), "pub struct Sibling;")?;
        fs::write(root.join("src/outer/inline/leaf.rs"), "pub struct Leaf;")?;
        fs::write(
            root.join("src/nested/mod.rs"),
            r##"#[path = "child.rs"] mod child;"##,
        )?;
        fs::write(root.join("src/nested/child.rs"), "pub struct Child;")?;
        let mut scan = Scan::default();
        scan.file(
            &root,
            &root.join("src/lib.rs"),
            "crate",
            &root.join("src"),
            &[],
            true,
        )?;
        for (name, module, file) in [
            ("Sibling", "crate::outer::explicit", "src/sibling.rs"),
            (
                "Leaf",
                "crate::outer::inline::leaf",
                "src/outer/inline/leaf.rs",
            ),
            ("Child", "crate::nested::child", "src/nested/child.rs"),
        ] {
            assert!(
                scan.declarations
                    .iter()
                    .any(|d| d.name == name && d.module == module && d.file == file)
            );
        }
        assert_eq!(
            scan.warnings,
            [
                "Unresolved module crate::outer::missing: src/missing.rs",
                "Unexpanded include! at src/outer.rs:4",
            ]
        );
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn dependency_edges_preserve_kind_target_and_optionality() {
        let manifest: toml::Value = toml::from_str("[dependencies]\ncore = { package = 'xgpui-core', version = '1', optional = true }\n[dev-dependencies]\ngpui = '1'").expect("fixture parses");
        let mut result = vec![];
        dependencies(
            &manifest,
            "cfg(unix)",
            &BTreeMap::from([
                ("gpui".into(), "gpui".into()),
                ("xgpui-core".into(), "gpui".into()),
            ]),
            &mut result,
        );
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].package, "gpui");
        assert_eq!(result[0].alias, "core");
        assert!(result[0].optional);
        assert_eq!(result[1].kind, "dev");
        assert_eq!(result[0].target, "cfg(unix)");
    }

    #[test]
    fn source_markup_cannot_inject_html() {
        assert_eq!(html("<script>\"&"), "&lt;script&gt;&quot;&amp;");
    }
}
