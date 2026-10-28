# xgpui-web

Browser windowing and rendering backend for **xgpui**, with WebGPU and a
WebGL2 fallback.

## When to use this crate

Build for `wasm32-unknown-unknown` and package the application with wasm-bindgen.
`xgpui-platform` provides `web_init`, backend selection and application
construction. WebGPU requires a supported browser and HTTPS or localhost.

For application setup, use the matching framework packages:

```toml
[dependencies]
gpui = { package = "xgpui-core", version = "1.0.0" }
gpui_platform = { package = "xgpui-platform", version = "1.0.0" }
```

The registry package is `xgpui-web`; its Rust library name is
`gpui_web`. Keep framework packages on a compatible xgpui release.

## Features

| Feature | Purpose |
| --- | --- |
| `multithreaded` | WASM threading support; enabled by default. |

For a single-threaded application, use the platform facade’s
`single_threaded_web()` entry point and the feature configuration described in
the [browser example source](https://github.com/gpui-luma-dev/xgpui/tree/main/gpui/crates/gpui-web/examples/hello_web).

## Documentation

- [Local development and platform prerequisites](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/LOCAL_DEVELOPMENT.md)
- [xgpui API additions and compatibility](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/gpui-public-api.md)
- [Release preparation](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/PUBLISHING.md)
- [Repository](https://github.com/gpui-luma-dev/xgpui) · [Website](https://xgpui.dev/)

## Versioning and provenance

The xgpui framework version is `1.0.0`, independent of the upstream release.
This maintained downstream package derives from Zed's `gpui_web` crate via
[gpui-unofficial](https://github.com/gpui-archipelago/gpui-unofficial).
The accepted capture is `release/1.25.3` at commit
`39d276b1500d503ec6d37efcaaa0dc0c00730576`; its Zed source is `v1.25.3`
(`8dd255c3eafb5231056fe4e6f601cedd011f1c93`).

Licensed under Apache-2.0. xgpui is independent of Zed Industries.
