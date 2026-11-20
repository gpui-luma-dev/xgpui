# XGPUI extension lab

[Read the lab guide](https://xgpui.dev/extension-lab.html) for the package
overview, target list, unpublished status and browser workflow.

An unpublished development package containing native labs, benchmarks and browser
demos. The main package uses one Cargo manifest and lockfile. Extension libraries stay in
`extensions/crates/`; their dependency graphs do not include this package.

The package is version `0.2.0` with `publish = false`. Publishing can be enabled
later after checking the package's bundled assets and consumer requirements.

From the repository root:

```sh
just labs                   # Pick a lab, then a view or task, from a menu.
just media                  # Open a lab directly.
just media tiled            # Open one of its views.
just media capture          # Run one of its tasks.
just labs media             # List the media lab's views and tasks.
just labs build             # Build native labs and benchmarks.
just labs test              # Run unit/compatibility tests.
just labs check             # Check labs.toml against Cargo.toml and the justfile.
just pages-demos            # Build and package both browser demos.
```

You only need to remember `just labs`: it lists the labs for this OS, then the
chosen lab's views and tasks, and prints the short command (`just media capture`)
before running it.

[`labs.toml`](labs.toml) is the single list of labs: each lab's binary or
example, its views (named flag presets) and its tasks (captures, benchmarks,
platform tests). The launcher in `tools/lab` reads it. `just labs check`, run in
CI, fails when a `[[bin]]` or example here isn't registered, or a lab has no
`just <lab>` recipe. To add a lab, view or task, edit `labs.toml` (and add the
one-line recipe for a new lab); there are no per-lab justfiles.

Arguments after a lab, view or task go to the app, including quoted file paths;
`just media -- --help` shows a lab's options. `just labs -n media capture` prints
the command without running it, and `just labs --profile dev media` builds a
debug binary. Every lab runs with runtime shaders, portable shaders and wgpu;
platform-specific capabilities still apply. The older names (`lab-media`,
`media-lab`, `paints`, `shaders`, …) remain aliases.

`just color` includes Swatches/Chromaticity tabs; see [Color lab](color-lab/README.md).

Select one executable directly:

```sh
cargo run --release --locked --config .cargo/validation.toml --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin paint-lab --target-dir target
```

Native binaries use the default `native` feature. Browser scripts build only the
selected WASM binary with `--no-default-features --features web`; generated JS/WASM
names and the hosted `/examples/extensions/` and `/examples/shaders/`
destinations remain unchanged. Source, fixtures and benchmark reports are grouped
by target under this package.

Tests that create real platform windows (`native_windows`, the Windows, Linux and
macOS window probes) are opt-in through their `*-window-tests` features, and
`just labs test` doesn't enable them. Run them with `just window-probes <probe>`;
`just labs window-probes` lists the probes for this OS. `just labs check` fails if a
window test isn't registered.

The [clip-chain feasibility lab](clip-chain-probe/README.md) (#37) keeps its isolated
GPU experiment in a separate unpublished Cargo package under this lab directory.
Use `just clip-chain-probe test`, `just clip-chain-probe wgpu`,
`just clip-chain-probe metal` (macOS), or `just clip-chain-probe browser`.
Its proposal and integration plan are in [docs/issue-37](../../docs/issue-37/README.md).
