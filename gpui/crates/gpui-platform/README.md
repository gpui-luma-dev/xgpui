# xgpui-platform

Platform selection and application construction for **xgpui**. This facade
chooses the native backend for macOS, Windows, Linux or FreeBSD, and the browser
backend for WebAssembly.

## Application setup

```toml
[dependencies]
gpui = { package = "xgpui-core", version = "1.0.0" }
gpui_platform = { package = "xgpui-platform", version = "1.0.0" }
```

The registry package is `xgpui-platform`; Rust code imports `gpui_platform`.
Start an ordinary application with `gpui_platform::application()`. Use
`gpui_platform::headless()` for a headless application, or `current_platform`
when supplying your own application construction.

For a complete window and render example, see
[hello_world.rs](https://github.com/gpui-luma-dev/xgpui/blob/main/gpui/crates/gpui/examples/hello_world.rs).

## Browser applications

Call `web_init()` from the WASM entry point. `application_with_web_backend()`
selects a browser renderer, while `single_threaded_web()` constructs an
application without the multithreaded entry point. Browser applications need
WASM packaging and a compatible server configuration; see the local-development
guide rather than treating a native executable as a browser build.

## Features

| Feature | Purpose |
| --- | --- |
| `wayland`, `x11` | Forward Linux window-system support. |
| `font-kit` | Forward macOS font discovery. |
| `runtime_shaders` | Forward runtime Metal shader compilation. |
| `screen-capture` | Forward supported native screen capture integration. |
| `test-support`, `bench-support` | Test and benchmark helpers. |

Platform behavior and native prerequisites remain backend-specific.

## Documentation

- [Local development and platform prerequisites](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/LOCAL_DEVELOPMENT.md)
- [xgpui API additions and compatibility](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/gpui-public-api.md)
- [Release preparation](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/PUBLISHING.md)
- [Repository](https://github.com/gpui-luma-dev/xgpui) · [Website](https://xgpui.dev/)

## Versioning and provenance

xgpui framework packages use `1.0.0`; this is independent of the upstream
`release/1.25.3` baseline. Derived from Zed's `gpui_platform` through
[gpui-unofficial](https://github.com/gpui-archipelago/gpui-unofficial), captured at
`39d276b1500d503ec6d37efcaaa0dc0c00730576` from Zed `v1.25.3`
(`8dd255c3eafb5231056fe4e6f601cedd011f1c93`).

Licensed under Apache-2.0. xgpui is independent of Zed Industries.
