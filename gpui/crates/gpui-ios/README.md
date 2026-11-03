# xgpui-ios (experimental)

Experimental local UIKit backend for XGPUI, captured from gpui-unofficial
`release/1.26.2` and originally developed in Zed. Version 0.1.0 is proposed as
its first experimental registry release; publication is pending approval.
Its APIs and platform coverage are not yet a supported compatibility contract.

It provides application lifecycle, windowing, native text input, safe-area and
keyboard insets, and touch input using the shared XGPUI Apple renderer.
Applications select it through `gpui_platform` on iOS. The standalone simulator
example is in `examples/ios/`; its build script requires Xcode and an installed
iOS Simulator runtime. For local checks, enable `runtime_shaders` to avoid
requiring precompiled Metal libraries.

The upstream example and tests are retained for validation. Simulator compilation
and execution must be recorded separately before claiming platform support.

Licensed under Apache-2.0; upstream notices are preserved in `LICENSE-APACHE`.
