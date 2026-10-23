# gamut-lab

`xgpui-gamut` producer + macOS Metal lab: wide P3 content and normal sRGB controls in one window.

```sh
just --justfile apps/gamut-lab/justfile ui
just --justfile apps/gamut-lab/justfile baseline
just --justfile apps/gamut-lab/justfile metal-test
```

[Design, compositing contract and run notes](../../docs/roadmap/extensions/high-gamut-windows.md).
Uses per-window public presentation APIs. HDR and other wide presentation backends remain deferred.
