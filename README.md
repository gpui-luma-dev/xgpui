# xgpui

An experimental GPUI repository for documentation, testing and future rendering
work, including custom shaders and wide-gamut photo editing.

The starting point is [gpui-unofficial](https://github.com/gpui-archipelago/gpui-unofficial),
which captures GPUI and its supporting crates from
[Zed](https://github.com/zed-industries/zed).

**[Read the documentation →](https://gpui-luma-dev.github.io/xgpui/)**

## How it works

- Releases are captured under `gpui/`. `upstream.json` records the release,
  source commit and package locations.
- The manual **Sync GPUI release** workflow imports a selected release onto
  `upstream`, keeping that branch separate from local development.
- Release updates are reviewed and integrated into `main`. Documentation,
  tooling and experimental changes live alongside the captured code.

## Build

From the repository root:

```sh
cargo build --locked
```

This builds the libraries supported by the selected target; it does not launch
an application. See [build instructions](docs/BUILDING.md) for prerequisites
and options.

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

## License

[Apache License 2.0](LICENSE), matching the captured gpui-unofficial crates.
Upstream copyright and license notices are preserved.
