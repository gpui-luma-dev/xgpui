# xgpui

GPU-accelerated application framework for **xgpui**, maintained downstream from
Zed's GPUI. It provides the application model, entities, windows, elements,
layout, input, text and scene primitives.

## Application setup

```toml
[dependencies]
gpui = { package = "xgpui", version = "1.0.0" }
gpui_platform = { package = "xgpui-platform", version = "1.0.0" }
```

The registry package is `xgpui`; its Rust library name remains `gpui`.
`xgpui-platform` selects and constructs the host backend.

A view implements `Render` and returns elements:

```rust,ignore
use gpui::{prelude::*, *};

struct Hello;

impl Render for Hello {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().p_4().child("Hello, xgpui!")
    }
}

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| Hello))
            .expect("could not open the application window");
    });
}
```

See the [complete hello-world example](https://github.com/gpui-luma-dev/xgpui/blob/main/gpui/crates/gpui/examples/hello_world.rs)
for font loading, window configuration and browser entry points.

## Optional extensions

| Package | Adds |
| --- | --- |
| `xgpui-color` | Standalone CPU color science and WGSL color math. |
| `xgpui-gamut` | Explicit wide-gamut SDR content producers. |
| `xgpui-shader` | Brushes, gradients, shader paints and layer effects. |
| `xgpui-media` | Image loading, media textures and frame presentation. |
| `xgpui-parley` | Parley/HarfRust text shaping and rasterization. |
| `xgpui-svg` | Progressive SVG elements and path effects. |

Extensions use independent `0.2.0` versions and opt-in APIs. Rendering extensions
require xgpui's downstream integration changes; do not substitute an unrelated
GPUI package with the same Rust import name. `xgpui-color` also works independently.

## Platform support

Native backends are separate packages selected through `xgpui-platform`.
Install the host prerequisites in the local-development guide. WebAssembly uses
`xgpui-web`, WASM packaging, and WebGPU or WebGL2; individual extension features
have their own platform limits.

## Documentation

- [Local development and platform prerequisites](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/LOCAL_DEVELOPMENT.md)
- [xgpui API additions and compatibility](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/extensions/gpui-public-api.md)
- [Release preparation](https://github.com/gpui-luma-dev/xgpui/blob/main/docs/PUBLISHING.md)
- [Repository](https://github.com/gpui-luma-dev/xgpui) · [Website](https://xgpui.dev/)

## Versioning and provenance

The xgpui framework version is `1.0.0`; it does not mirror Zed's version.
The accepted baseline is `release/1.25.3` from
[gpui-unofficial](https://github.com/gpui-archipelago/gpui-unofficial), commit
`39d276b1500d503ec6d37efcaaa0dc0c00730576`. Its original source is
[Zed's GPUI](https://github.com/zed-industries/zed/tree/v1.25.3/crates/gpui),
Zed `v1.25.3` at `8dd255c3eafb5231056fe4e6f601cedd011f1c93`.
xgpui maintains additional APIs and rendering behavior on top of that capture.

Licensed under Apache-2.0. xgpui is independent of Zed Industries.
