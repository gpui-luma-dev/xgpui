# xgpui-ext

A home for experimental GPUI extensions, developed alongside the local GPUI
capture. Each extension has its own crate, dependencies, API, and GitHub issue.
This directory is an umbrella, not a single extension crate.

## Structure

```text
extensions/xgpui-ext/
  crates/
    xgpui-shader/    Custom shader and canvas extension (Metal solid-color and mesh producers)
apps/                         # Repository-level app directory
  shader-lab/                 Producer correctness and performance app (macOS Metal)
  paint-lab/                  Extension paint coverage integration app (macOS Metal)
```

The shader integration, mesh producer, and local lab use standalone Cargo manifests and
local path dependencies. See [shader-lab](../../apps/shader-lab/README.md) for commands.
The root build manifests remain unchanged; follow
[the build instructions](../../docs/BUILDING.md) when integrating new crates.

## Boundaries

- Keep extension implementation here and GPUI integration changes small and
  explicit. Preserve existing application APIs wherever practical; identify and
  justify necessary compatibility breaks before implementation.
- Extensions may depend on GPUI. GPUI core must not depend on extension crates.
- Backend adapters may require integration in existing renderer crates. Keep
  backend GPU dependencies out of GPUI core solely for extension support.
- Keep unrelated capabilities independent. High-gamut window presentation is
  separate from custom shader support.
- Provide a local test/performance app for extensions that change rendering.
  Measure against a baseline and document the measurement conditions.

## Current design work

- [#7: Custom shader and canvas architecture](https://github.com/gpui-luma-dev/xgpui/issues/7)
  is the first task. Its reference use case is an animated mesh gradient exposed
  through a small, styled GPUI component API.
- [#8: High-gamut window presentation](https://github.com/gpui-luma-dev/xgpui/issues/8)
  is independent; its eventual implementation location remains a design decision.

Agree objectives, integration contracts, crate dependencies, compatibility impact,
and acceptance criteria before coding either capability.

The [extension paint contract](docs/extension-paints.md) describes the additive
paint API and coverage integration. See [paint-lab](../../apps/paint-lab/README.md).
