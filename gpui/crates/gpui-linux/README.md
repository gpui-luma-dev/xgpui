# xgpui-linux

Linux and FreeBSD windowing and input backend for **xgpui**, with Wayland and
X11 integration and wgpu rendering.

## When to use this crate

`xgpui-platform` selects the host platform. Install the native libraries listed
in the local-development guide before building on Linux. Rendering features and
window-system support must match the application environment.

For application setup, use the matching framework packages:

```toml
[dependencies]
gpui = { package = "xgpui", version = "1.0.0" }
gpui_platform = { package = "xgpui-platform", version = "1.0.0" }
```

The registry package is `xgpui-linux`; its Rust library name is
`gpui_linux`. Keep framework packages on a compatible xgpui release.

## Features

| Feature | Purpose |
| --- | --- |
| `wayland` | Wayland support; enabled by default. |
| `x11` | X11 support; enabled by default. |
| `screen-capture` | Optional screen capture integration. |

## Documentation

- [Local development and platform prerequisites](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/LOCAL_DEVELOPMENT.md)
- [xgpui API additions and compatibility](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/gpui-public-api.md)
- [Release preparation](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/PUBLISHING.md)
- [Repository](https://github.com/gpui-luma-dev/xgpui) · [Website](https://xgpui.dev/)

## Versioning and provenance

The xgpui framework version is `1.0.0`, independent of the upstream release.
This maintained downstream package derives from Zed's `gpui_linux` crate via
[gpui-unofficial](https://github.com/gpui-archipelago/gpui-unofficial).
The accepted capture is `release/1.25.3` at commit
`39d276b1500d503ec6d37efcaaa0dc0c00730576`; its Zed source is `v1.25.3`
(`8dd255c3eafb5231056fe4e6f601cedd011f1c93`).

Licensed under Apache-2.0. xgpui is independent of Zed Industries.
