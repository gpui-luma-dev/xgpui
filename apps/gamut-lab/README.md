# gamut-lab

macOS debug-only Metal prototype: wide P3 content and normal sRGB controls in one window.

```sh
just --justfile apps/gamut-lab/justfile ui
just --justfile apps/gamut-lab/justfile baseline
just --justfile apps/gamut-lab/justfile metal-test
```

[Design, compositing contract and run notes](../../docs/roadmap/extensions/high-gamut-windows.md).
No public high-gamut GPUI API; HDR and production support remain deferred.
