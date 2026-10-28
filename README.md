# xgpui

An extensible GPUI framework with custom shaders, wide-gamut rendering and tools
for building applications such as photo editors, backed by documentation and tests.

XGPUI started from [gpui-unofficial](https://github.com/gpui-archipelago/gpui-unofficial),
which captures GPUI and its supporting crates from
[Zed](https://github.com/zed-industries/zed). We are moving toward using Zed
directly as our upstream.

**[Read the documentation →](https://xgpui.dev/)**

## How it works

- Modified framework/backend crates are captured under `gpui/`; supporting
  crates use the matching published `gpui-unofficial` packages. `upstream.json`
  records the release, source commit, local packages and registry dependencies.
- The manual **Sync GPUI release** workflow imports a selected release onto
  `upstream`, keeping that branch separate from local development.
- Release updates are reviewed and integrated into `main`. Documentation,
  tooling and experimental changes live alongside the captured code.

## Build

From the repository root:

```sh
just         # List sample apps and development commands.
just build   # Build optimized native libraries.
just shaders # Launch the shader examples.
```

Recipes use Bash (on Windows, Git for Windows Bash must be on PATH).
Without just, use `cargo build --release --locked`.

`just lab-build` builds the unpublished extension labs and benchmarks from
`apps/xgpui-ext-lab/`. See [Labs & benchmarks](docs/extensions/extension-lab.md)
for targets, launch commands and why the lab package is unpublished.

`just build` builds the libraries supported by the selected target; it does not
launch an application. See [Local Development](docs/LOCAL_DEVELOPMENT.md) for Windows,
Linux and macOS build/run commands, or [build harness details](docs/BUILDING.md).

## Documentation website

```sh
cargo run --locked --manifest-path tools/crate-roadmap/Cargo.toml
```

Open `docs/index.html`. The site includes the crate map,
an architecture reference, an interactive rendering
pipeline explorer, and an **Extensions to GPUI** section. The extension section
distinguishes opt-in downstream APIs from upstream GPUI and documents the
`xgpui-shader` integration. It works offline. Generated pages are not committed.

See [documentation instructions](docs/WEBSITE.md) for maintenance and checks.

## Contributing

Contributions are welcome, including fully AI-generated code. You are responsible
for its quality. See [Contributing](CONTRIBUTING.md) for the short guide.

## License

[Apache License 2.0](LICENSE), consistent with GPUI from Zed.
Upstream copyright and license notices are preserved.
