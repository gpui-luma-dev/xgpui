# xgpui-ios

iOS backend for XGPUI, derived from gpui-unofficial `release/1.27.0` and
originally developed in Zed. The current downstream package version is **1.26.4**;
the selected family uses core 3.0.0 and Apple 4.0.0. The 1.27.0 integration
awaits final public API review and publication.

Dependency identities use XGPUI's core and shared Apple backend. Package metadata,
the simulator build script, and renderer initialization are adapted to this repository.

It provides application lifecycle, windowing, native text input, safe-area and
keyboard insets, and touch input using the shared objc2 Apple renderer.
Applications select it through `gpui_platform` on iOS. The standalone simulator
example is in `examples/ios/`; its build script requires Xcode. For local checks,
enable `runtime_shaders` to avoid requiring precompiled Metal libraries.

Validation includes simulator library/example compilation, app bundle packaging,
and native protocol-test compilation. Interactive simulator/device execution
remains pending; an installed simulator runtime is needed for that validation.
Wide-gamut SDR presentation currently remains macOS-only.

Licensed under Apache-2.0; upstream notices are preserved in `LICENSE-APACHE`.
