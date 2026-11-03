# xgpui-ios

iOS backend for XGPUI, captured from gpui-unofficial `release/1.26.2` and
originally developed in Zed. The package version is **1.26.2**, following
the captured upstream release while the iOS implementation remains unchanged.

The iOS source files are unchanged from that capture. Dependency identities use
XGPUI's core and shared Apple backend; package metadata and the simulator build
script are adapted to this repository.

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
