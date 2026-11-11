"""Check the static website in pages/: links, anchors, assets and legacy routes.

Reads checked-in files only; runs no build. Usage: python3 scripts/check_pages.py
"""

from fnmatch import fnmatch
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
PAGES = ROOT / "pages"
SITE = "https://xgpui.dev/"
# Written by apps/xgpui-ext-lab/*/build.sh; absent until `just web-build`.
GENERATED = ["examples/extensions/italy/*", "examples/extensions/icc/*", "examples/extensions/colorimetry/*",
             "examples/extensions/font-licenses/*[!l]", "examples/extensions/*.svg",
             "examples/extensions/svg-ATTRIBUTION.md"]
DEMOS = ["canvas", "mandelbrot", "shader", "brushes", "layers", "hermite", "parley", "font-controls", "svg",
         "color", "chromaticity", "pixel-metadata", "typography", "media-filters", "media-float", "media-tiles",
         "mipmaps", "path-effects"]
SHADER_EXAMPLES = ["minimal", "glow", "particles", "composition", "shadows", "borders"]
API_SECTION = "gpui-public-api-changes-additions-and-compatibility"

failures = []


def check(condition, message):
    if not condition:
        failures.append(message)


class Page(HTMLParser):
    def __init__(self, text):
        super().__init__(convert_charrefs=True)
        self.ids, self.links, self.title = set(), [], None
        self._in_title = False
        self.feed(text)

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if "id" in attrs:
            self.ids.add(attrs["id"])
        if tag == "a" and "name" in attrs:
            self.ids.add(attrs["name"])
        for key in ("href", "src"):
            if attrs.get(key) is not None:
                self.links.append((tag, key, attrs[key], attrs))
        self._in_title = tag == "title"

    def handle_data(self, data):
        if self._in_title:
            self.title = (self.title or "") + data

    def handle_endtag(self, tag):
        self._in_title = False


def rel(path):
    return path.relative_to(PAGES).as_posix()


pages = {rel(p): p for p in sorted(PAGES.rglob("*.html"))}
parsed = {name: Page(path.read_text()) for name, path in pages.items()}


def target_page(base, href):
    """Resolve a relative link to (pages-relative path, fragment) or None if outside pages/."""
    parts = urlsplit(href)
    path = (PAGES / Path(base).parent / unquote(parts.path)).resolve() if parts.path else PAGES / base
    try:
        name = path.relative_to(PAGES.resolve()).as_posix()
    except ValueError:
        return None, parts.fragment
    if path.is_dir() or parts.path.endswith("/"):
        name = (name + "/index.html").lstrip("./") if name != "." else "index.html"
    return name, parts.fragment


# 1. Every relative link and asset exists; every fragment names an element.
for name, page in parsed.items():
    if name != "404.html":
        check(page.title and page.title.strip(), f"{name}: missing <title>")
    for tag, key, href, attrs in page.links:
        scheme = urlsplit(href).scheme
        if scheme or href.startswith(("mailto:", "javascript:")):
            if "en.wikipedia.org/wiki/" in href:
                check(attrs.get("target") == "_blank" and "noopener" in attrs.get("rel", ""),
                      f"{name}: glossary link must open a new tab: {href}")
            continue
        if href.startswith("/"):
            check(name == "404.html", f"{name}: links must stay relative for every host prefix: {href}")
            continue
        target, fragment = target_page(name, href)
        if target is None:
            check(False, f"{name}: link leaves pages/: {href}")
            continue
        if any(fnmatch(target, pattern) for pattern in GENERATED):
            continue
        check((PAGES / target).is_file(), f"{name}: missing {key} target {href}")
        if fragment and target in parsed and not fragment.startswith("L"):
            check(fragment in parsed[target].ids, f"{name}: missing anchor {href}")
print(f"Checked links, assets and anchors in {len(parsed)} pages.")

# Links to repository files on GitHub must name files on this branch.
repo_links = 0
for name, page in parsed.items():
    for _, _, href, _ in page.links:
        found = re.match(r"https://github\.com/gpui-luma-dev/xgpui/(?:blob|tree)/main/([^?#]+)", href)
        if found:
            repo_links += 1
            check((ROOT / unquote(found.group(1))).exists(), f"{name}: GitHub link to missing file {href}")
print(f"Checked {repo_links} links to repository files.")

# 2. Each captured and extension crate has a page; index forwards every former route.
upstream = json.loads((ROOT / "upstream.json").read_text())
extensions = sorted(p.parent.name for p in (ROOT / "extensions/crates").glob("*/Cargo.toml"))
crates = sorted(upstream["packages"]) + extensions
check(sorted(n[7:-5] for n in pages if n.startswith("crates/")) == sorted(crates),
      "pages/crates/ must have exactly one page per captured and extension crate")
index = (PAGES / "index.html").read_text()
match = re.search(r"var pages = (\[.*?\]), crates = (\[.*?\]), repo = (\{.*?\});", index)
check(match, "index.html: legacy route forwarding is missing")
if match:
    routes, forwarded, repo_docs = (json.loads(group) for group in match.groups())
    guides = sorted(n[:-5] for n in pages if "/" not in n and n not in ("index.html", "404.html"))
    check(sorted(routes) == guides, "index.html: forwarded routes must match top-level pages")
    check(sorted(forwarded) == sorted(crates), "index.html: forwarded crates must match crate pages")
    for doc in repo_docs.values():
        check((ROOT / doc).is_file(), f"index.html: forwarded repository document missing: {doc}")
check(f"gpui-unofficial {upstream['ref']}." in index, "index.html: upstream baseline differs from upstream.json")
for name in pages:
    if name.endswith(".html") and name != "404.html" and not name.startswith("examples/"):
        rail = (PAGES / name).read_text()
        for crate in crates:
            check(f'crates/{crate}.html"' in rail, f"{name}: navigation is missing {crate}")
print(f"Checked {len(crates)} crate pages, navigation and legacy route forwarding.")

# 3. Extension crates document GPUI API changes in their guide and README.
api = parsed.get("gpui-public-api.html")
for crate in extensions:
    if crate == "xgpui-color":  # standalone math, no GPUI dependency
        continue
    crate_page = PAGES / "crates" / f"{crate}.html"
    guide = re.search(r'class="crate-guide"><a href="\.\./([\w.-]+\.html)"',
                      crate_page.read_text() if crate_page.is_file() else "")
    check(guide and guide.group(1) in parsed, f"{crate}: crate page must link its guide (class=\"crate-guide\")")
    if not guide or guide.group(1) not in parsed:
        continue
    page = parsed[guide.group(1)]
    readme = (ROOT / "extensions/crates" / crate / "README.md").read_text()
    sections = re.findall(r"xgpui\.dev/gpui-public-api\.html#([\w-]+)", readme)
    check("## GPUI public API changes: additions and compatibility" in readme and sections,
          f"{crate}: README needs the GPUI API compatibility section and link")
    check(API_SECTION in page.ids, f"{crate}: {guide.group(1)} needs the GPUI API compatibility section")
    for section in sections:
        check(api and section in api.ids, f"{crate}: unknown gpui-public-api section {section}")
        check(any(h.endswith(f"gpui-public-api.html#{section}") for _, _, h, _ in page.links),
              f"{crate}: {guide.group(1)} must link gpui-public-api.html#{section}")
print("Checked GPUI API compatibility sections for extension crates.")

# 4. Repository Markdown links into the website resolve.
markdown = [p for p in ROOT.rglob("*.md") if not any(part in ("target", ".git", "node_modules") for part in p.parts)]
site_links = 0
for path in markdown:
    for href in re.findall(r"https://xgpui\.dev/([^\s)\"'<>`*]*)", path.read_text(errors="ignore")):
        site_links += 1
        target, fragment = target_page("index.html", href.rstrip(".,"))
        where = path.relative_to(ROOT)
        if target is None or any(fnmatch(target, pattern) for pattern in GENERATED):
            continue
        check((PAGES / target).is_file(), f"{where}: broken website link {href}")
        if fragment and target in parsed:
            check(fragment in parsed[target].ids, f"{where}: broken website anchor {href}")
print(f"Checked {site_links} website links in repository Markdown.")

# 5. Demo galleries list every demo, backend and Wasm module.
demo_index = (PAGES / "examples/extensions/index.html").read_text()
shader_index = (PAGES / "examples/shaders/index.html").read_text()
hello = ROOT / "gpui/crates/gpui-web/examples/hello_web"
modules = re.findall(r'data-wasm-module="([^"]+)"', demo_index)
check(sorted(modules) == sorted(re.findall(r'\[\[bin\]\]\s+name = "([^"]+)"', (hello / "Cargo.toml").read_text())),
      "examples/extensions: every legacy Wasm binary needs an index entry")
for module in re.findall(r'data-module="([^"]+)"', (hello / "index.html").read_text()):
    check(module in modules, f"examples/extensions: legacy gallery module {module} is not listed")
for demo in DEMOS:
    for suffix in ("", "&backend=webgpu", "&backend=webgl"):
        check(f'href="?demo={demo}{suffix}"' in demo_index, f"examples/extensions: missing ?demo={demo}{suffix}")
    check(f'"{demo}"' in demo_index, f"examples/extensions: loader does not accept {demo}")
for example in SHADER_EXAMPLES:
    for suffix in ("", "&backend=webgpu", "&backend=webgl"):
        check(f'href="?example={example}{suffix}"' in shader_index, f"examples/shaders: missing {example}{suffix}")
        check(f'href="../shaders/?example={example}{suffix}"' in demo_index,
              f"examples/extensions: missing shader link {example}{suffix}")
launches = re.findall(r'<a href="([^"]+)">Open demo ↗</a>', demo_index)
check(len(launches) == len(set(launches)) == 25, "examples/extensions: expected 25 distinct demo launches")
for path in re.findall(r'href="https://github\.com/gpui-luma-dev/xgpui/blob/main/([^"#]+)', demo_index):
    check((ROOT / path).exists(), f"examples/extensions: source link target missing: {path}")
for html in (demo_index, shader_index):
    check("from '../graphics.mjs'" in html and "canonicalLocalDemoUrl(location.href)" in html,
          "demo pages must load shared graphics diagnostics")
print("Checked demo galleries, backend links and source targets.")

# 6. Benchmark reports linked from guides are complete observations.
for path in sorted((PAGES / "benchmarks").glob("*.json")):
    report = json.loads(path.read_text())
    for observation in report.get("observations", []):
        for phase, values in observation.items():
            if isinstance(values, dict) and "raw_ms" in values:
                raw = values["raw_ms"]
                check(raw and all(isinstance(v, (int, float)) and v >= 0 for v in raw),
                      f"{path.name}: {phase} needs non-negative samples")
                if "max_ms" in values and raw:
                    check(values["max_ms"] == max(raw), f"{path.name}: {phase} max_ms differs from samples")
        check(observation.get("errors", []) == [], f"{path.name}: observation recorded errors")
check("MIT LICENSE" in (PAGES / "vendor/prism/LICENSE").read_text(), "Prism license missing")
print("Checked benchmark reports and vendored licenses.")

if failures:
    print(f"\n{len(failures)} problem(s):", file=sys.stderr)
    for failure in failures:
        print(f"  {failure}", file=sys.stderr)
    sys.exit(1)
print("Website checks passed.")
