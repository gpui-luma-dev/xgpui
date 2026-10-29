# Crates.io release preparation

Tracking: [issue #49](https://github.com/gpui-luma-dev/xgpui/issues/49).
This is the initial inventory, not a release-ready declaration or publication
workflow. Actual first publication needs explicit user authorization.

## Current package policy

`scripts/release-packages.json` already maps nine framework packages to the
`xgpui`/`xgpui-*` names at `1.0.0` and six extension packages to `0.2.0`. Rust import
aliases remain separate from registry package names (for example, `gpui` imports
the `xgpui` package). Upstream `release/1.25.3` is provenance, not the xgpui
release version. Keep using this map rather than introducing another version list.

On 2026-10-28, read-only requests to `https://crates.io/api/v1/crates/NAME`
returned HTTP 404 for the 15 candidate names then configured, before the core
package was renamed to `xgpui`. No registry lookup for `xgpui` has been performed
as part of this rename. This is a point-in-time lookup,
not a reservation, a guarantee that a name may be published, or proof of ownership.
Check again before the first release and settle the publishing account/team.

## Core development dependency and first registry dry run

`xgpui-macros 1.0.0` has been published. The `xgpui` package keeps its
`xgpui-platform` development dependency as a local path without a registry
version. The platform itself depends on core, so requiring its registry version
while packaging core would create a circular first-publication requirement.
Cargo omits this path-only development dependency from the published manifest;
repository tests and examples continue to use the local platform crate.
Normalization preserves this policy, and the preflight audit accepts path-only
development dependencies while still validating their local paths. The reported
local development graph can therefore contain cycles that are absent from the
published dependency graph. Runtime/build dependency version pins are unchanged.

On 2026-10-28, a fresh isolated `cargo publish --dry-run --registry crates-io`
for `xgpui 1.0.0` passed packaging and compilation without repository patches or
upload credentials. Archive inspection verified all 178 files, the omission of
`xgpui-platform` from both the normalized manifest and packaged lockfile, and
resolution of `xgpui-macros 1.0.0` from crates.io. Full commands and evidence:
`/private/tmp/xgpui-local-platform-dev-dry-run/report.json`. The core was not
uploaded. Tests/examples that use the local platform remain checkout workflows.

## Run the local inventory

Requires Python 3.11 or later, with no third-party dependencies:

```sh
python3 scripts/publish_preflight.py > /tmp/xgpui-publish-preflight.json
python3 -m unittest discover -s scripts -p 'test_publish_preflight.py' -v
```

The script reads the release map, manifests and repository patch configuration.
It preserves dependency aliases, kinds, target conditions and optional flags;
checks the current exact internal version pins and local paths; reports metadata,
Git sources, publication restrictions and patches; and computes dependency-first
layers. It does not access credentials or the network, change manifests, package
crates, or publish. Exit success means the inventory ran, not that publishing is
ready. `release_ready` deliberately remains false until the remaining gates are
implemented and evidenced.

The graph conservatively includes every optional/target dependency, without
evaluating features or cfg. Development dependencies are excluded from the first
ordering and included in a separate cycle check. Reported cycle-blocked nodes
include dependents of cycles, not just cycle members. This is a manifest inventory,
not a resolved Cargo graph or a validated publish execution order.

## Initial findings

- All 15 release candidates now have explicit descriptions, Apache-2.0 licensing,
  repository, homepage, documentation, README, keywords and categories. Metadata
  uses `https://xgpui.dev/` for the homepage and documentation, matching the
  deployed GitHub Pages domain, and `https://github.com/gpui-luma-dev/xgpui`
  for the repository. All crate README website links use the same domain.
- All six extensions retain `publish = false`. Framework publication flags and
  package versions are unchanged. Complete source/license and asset attribution
  review before enabling any extension publication.
- All 15 package READMEs now describe xgpui package identities, usage and feature
  boundaries. Framework READMEs preserve the Zed/gpui-unofficial provenance while
  replacing inherited package names and upstream-mirrored version instructions.
  Extension READMEs summarize user-facing capabilities and link to the detailed
  guides, platform limits and compatibility contracts. Registry examples describe
  the intended release versions; isolated consumer validation remains required.
- `xgpui-linux` uses a Git revision of `zed-xim` with a registry version. Verify
  the registry fallback provides the required code/features.
- `xgpui-wgpu` has a Git-only `naga` development dependency. Decide how packaged
  tests are validated; do not treat development edges as ordinary publish ordering.
- Local builds use `block`, `async-process` and `proptest` compatibility patches.
  Root/local configuration cannot establish that registry consumers will work.
  Audit their actual transitive use and solve demonstrated compatibility gaps.
- Framework manifests still consume supporting `*-gpui-unofficial` crates at
  `1.25.3`. Own extraction and retained dependency policy (#18/#47) must settle
  before claiming an independent, self-contained release.
- The first static graph has no normal/build cycles. Including development edges
  introduces cycles. Record the chosen packaging/test strategy before automating.

Initial conservative dependency-first layers:

| Layer | Packages |
| --- | --- |
| 1 | xgpui-color, xgpui-macros |
| 2 | xgpui |
| 3 | xgpui-apple, xgpui-svg, xgpui-wgpu, xgpui-windows |
| 4 | xgpui-linux, xgpui-macos, xgpui-shader, xgpui-web |
| 5 | xgpui-gamut, xgpui-media, xgpui-platform |
| 6 | xgpui-parley |

An initial `cargo package --offline --list` on a scratch copy of `xgpui-color`,
outside this repository's Cargo configuration, succeeded and listed
`src/math.wgsl`, Rust sources, README and tests. It warned about the missing
description/license. This did not compile an extracted package, verify licensing,
or prove publication readiness. Use scratch copies for further package probes:
even a list command can resolve dependencies and rewrite lockfiles.

## Isolated xgpui-color validation

Run the repeatable check from the repository root with cached dependencies:

```sh
python3 scripts/verify_color_package.py --offline --out /tmp/xgpui-color-check
```

The output directory must be new and outside this repository. Omit `--offline`
only if registry dependencies need downloading. On macOS with an accessible Metal
GPU, include the native compute comparison:

```sh
python3 scripts/verify_color_package.py --offline --native-metal --out /tmp/xgpui-color-metal-check
```

The default run explicitly skips `metal_float32_matches_cpu_reference`; it does
not claim native GPU validation. The `--native-metal` run requires device access
but opens no GUI. The report records that choice and the host toolchain.

The helper stages a copy without the repository lockfile or Cargo configuration,
uses a fresh `CARGO_HOME` and target directory, and shares only the registry
cache. Cargo environment overrides and compiler flags/wrappers are removed.
It packages and verifies the library, checks archived file contents against the
staged source, and runs tests on a separately extracted archive. It then copies
`scripts/fixtures/color-consumer` into scratch space and points that consumer at
the extracted archive while retaining the candidate version requirement. Metadata
must resolve only the consumer and `xgpui-color`, with no checkout dependencies.
The consumer runs public conversion, gamut-mapping and gradient APIs and reads
WGSL through the exported constant. No registry upload is performed.

Evidence on 2026-10-28, macOS aarch64, Rust/Cargo 1.99.0:

- `cargo package` verification passed without any repository patches.
- The 12-file archive includes the README, `LICENSE-APACHE`, all Rust source/test
  files and `src/math.wgsl`. The license notice matches GPUI's `LICENSE-APACHE`.
- All 14 CPU tests, WGSL translation, native Metal comparison and two README
  doctests passed from the extracted archive.
- The separate consumer compiled and ran; its resolved graph contained only
  itself and `xgpui-color`, confirming no external runtime dependencies.
- Archive SHA-256: `0bd981acfc64350866d900e65df164c6348febe8e5762bbefddb9a74484ab7eb`.
- Full report and logs: `/private/tmp/xgpui-color-release-validation/report.json`.
  Each future run records its own commands, logs, file list and archive checksum.

The initial sandboxed attempt could not access a Metal device; the final native
run passed with device access. The registry `block` dependency used by the Metal
test tooling emitted a future-compatibility warning on this toolchain; it was not
a build failure and is absent from the standalone runtime consumer graph.
Windows/Linux host validation and installation from crates.io are not claimed.
`publish = false` remains set.

## Isolated validation of the full candidate set

The reusable harness is `scripts/verify_release_packages.py`. It validates all
15 archives so the remaining 14 consumers can resolve candidate dependencies:

```sh
python3 scripts/verify_release_packages.py --offline --out /tmp/xgpui-release-check
```

Use a new output directory outside the checkout. Cached registry dependencies
are needed for `--offline`; omit that flag when downloads are needed. A nonzero
exit with `completed-with-blockers` means the report contains failing checks,
not that every package failed. Logs and candidate archive checksums are retained.

The archive phase runs `cargo package --no-verify --exclude-lockfile`: excluding
lockfile generation avoids requiring the unpublished candidate versions to
already exist in the registry. This is **archive generation and inspection**,
not Cargo's default package/build verification. Final release packaging still
needs registry-equivalent resolution and its chosen lockfile policy.

The harness stages the original directory layout, packages the unchanged
candidate manifests, inspects code, shaders, resource inputs, test fonts and
license notices, and extracts pristine archives. Cargo intentionally excludes
nested packages such as `hello_web`; those are not required runtime inputs of
`xgpui-web`. Validation-only copies receive paths to other extracted candidates,
retaining dependency aliases, exact versions, features, target conditions and
kinds. Third-party dependencies are not patched. Full consumer metadata confirms
candidate resolution comes from those extracted copies, never this checkout.
The consumer lockfiles, Cargo configuration and targets are isolated; only the
registry cache is shared.

### Results recorded on 2026-10-28

These scratch reports and archives predate the approved core-package rename to
`xgpui`. They establish the earlier source validation, not validation of newly
named release artifacts. Rerun packaging with the updated release map before
publishing. Rust imports remain `gpui`; package versions are unchanged.

All **15 archives passed** content inspection. Added matching GPUI
`LICENSE-APACHE` notices to the remaining extension packages; the shader's
existing mesh-gradient notice and Parley's font licenses remain included.

| Candidate | Consumer result |
| --- | --- |
| xgpui | macOS compile check passed. |
| xgpui-apple | macOS default and `font-kit` checks passed. |
| xgpui-macos | macOS compile check passed. |
| xgpui-macros | Compile check and a derive invocation in the native executable passed. |
| xgpui-platform | macOS compile check passed. |
| xgpui-wgpu | macOS compile check passed. |
| xgpui-color | Dedicated package/tests/consumer passed; also used in the native executable. |
| xgpui-gamut | macOS default and `instanced` checks passed. |
| xgpui-media | macOS default and `wgpu` checks passed. |
| xgpui-parley | macOS default and `system-fonts` checks passed. |
| xgpui-shader | macOS default and `portable-shaders` checks passed. |
| xgpui-svg | macOS compile check passed. |
| xgpui-linux | Cross-check blocked by the missing Linux C sysroot (`stdio.h` while compiling Wayland). |
| xgpui-windows | Cross-check blocked by the missing `llvm-rc` Windows resource compiler. |
| xgpui-web | Single-threaded profiles passed on stable; default multithreaded profile passed on nightly with atomic-WASM flags. |

A combined consumer using all 12 macOS-applicable packages **built, linked and
ran**. It derives an element through the macro crate, exercises pure public
color/brush/gamut/filter/scheduling APIs, and references the platform/backend
APIs without creating an application or window. It uses no repository patches
or global Rust compiler flags. Individual package checks use default features;
the combined executable additionally enables shader brushes and instanced gamut.
This is not native GUI rendering validation or full test-suite coverage.

The macOS checks used stable Rust/Cargo 1.99.0. Initial sandbox failures in
Metal's compiler cache were resolved by permitting native compiler access; these
were not package-content failures.

### Browser configurations and completion ownership

The default browser dependency enables multithreading and requires nightly Rust,
`rust-src`, `build-std=std,panic_abort`, and the atomic-WASM flags in the standalone
browser example. That isolated consumer now compiles with nightly
`1.101.0-nightly (41089d07a 2026-10-27)`.

The bare single-threaded dependency also compiles on stable after the completion
ownership fix:

```toml
[dependencies]
gpui_web = { package = "xgpui-web", version = "=1.0.0", default-features = false }
```

The maintained lab's application configuration, which additionally enables wgpu's
`fragile-send-sync-non-atomic-wasm` feature, was checked separately and also passes.
That feature retains its non-atomic, single-threaded contract; it is not required
by the bare single-threaded consumer above or used in the atomic nightly check.
These checks compile consumer libraries; browser execution is not claimed.

Browser upload completion now retains concrete staging leases instead of erasing
them behind `Arc<dyn Send + Sync>`. Native queue callbacks continue to own and
release these leases directly. WASM keeps leases in thread-local pending storage;
the queue callback carries only an atomic completion signal. Completed leases
are collected on their owner thread during the callback or the next preparation
or staging operation. Drops run outside the storage borrow so admission guards
can safely run user code. No unsafe thread-safety assertion or non-atomic wgpu
feature is used to make the multithreaded profile compile.

The custom-draw test group passed all 12 tests, including native staging reuse,
abandoned recordings, the native completion `Send + Sync` contract, and a new
owner-thread retention test with a completion signal sent from another thread.

For a repeatable nightly probe (install official prerequisites first):

```sh
rustup toolchain install nightly --profile minimal --component rust-src --target wasm32-unknown-unknown
python3 scripts/verify_release_packages.py --offline --out /tmp/xgpui-release-nightly-check --nightly-web
```

On an existing snapshot, `--retry-consumers` retries failed consumers and retains
prior results/logs. Add `--profiles-only` to leave baseline consumer checks alone
while running pending feature profiles. The native fixture is rerun if its input
files change. Snapshots retain the archived source; start a fresh output directory
after editing candidate sources or manifests. Nightly's first `build-std` run may
need registry downloads before an offline rerun can work.

Full evidence for the repaired browser build and repeated archive/native checks:
`/private/tmp/xgpui-release-wasm-fix-validation/report.json`.
Native Windows/Linux builds, full optional-feature
and development-test coverage, and registry installation remain open release
gates. Publication flags are unchanged and no upload was performed.

## Next implementation slice

1. Confirm the existing names/version split and publishing ownership. Start with
   `xgpui-color`: it has no runtime dependencies and provides a small packaging
   fixture. Its macOS isolated archive and consumer validation now passes; retain
   `publish = false` while collecting the remaining release evidence.
2. The full candidate archive and native-consumer pass is now recorded above.
   The multithreaded WASM compile blocker is repaired. Collect native Windows/Linux
   evidence and validate the remaining optional/development configurations. Address any
   demonstrated compatibility-patch and development-cycle gaps explicitly.
3. Record evidence for #18, #47 and gpui-luma dogfooding (#48), including Windows,
   Linux and macOS. Browser checks are secondary. These issues remain open; this
   inventory does not satisfy their release gates.
4. Only after validation is established, add a separately triggered release
   workflow with registry authentication, ordered/resumable uploads and
   post-publication checks. No publishing workflow is added by this first slice.

## Isolated packaging after the core rename

A fresh, offline run with the `xgpui` identity completed on 2026-10-28:

```sh
python3 scripts/verify_release_packages.py --offline --nightly-web --out /tmp/xgpui-renamed-release-check
```

All 15 archives passed inspection, including `xgpui-1.0.0.crate`. All 11
macOS-applicable per-crate consumer checks passed, and the combined native
consumer built, linked and ran using the renamed core. Both single-threaded
browser profiles passed on stable; the default multithreaded profile passed on
nightly with the standalone example's atomic-WASM flags. Candidate dependencies
resolved only from extracted validation copies, retaining version requirements
and the `gpui` Rust alias. No repository patches or registry requests were used.

Full evidence: `/private/tmp/xgpui-renamed-release-validation/report.json`.
The report retains the known local Linux/Windows cross-toolchain failures and
the expected stable-toolchain failure for default threaded WASM; its aggregate
status is therefore `completed-with-blockers`. These are not newly demonstrated
rename failures. Existing Windows/Linux platform coverage was accepted by the
project owner. Archive generation still uses `--no-verify --exclude-lockfile`;
registry-backed package verification and browser runtime execution are not
claimed by this run.

The offline preparation command's separate archive pass also inspected all 15
archives successfully. Its report is
`/private/tmp/xgpui-renamed-preparation-bundle/release-plan.json`.

## Offline release preparation and approval boundary

The core registry package is `xgpui` at `1.0.0`; its Rust library alias remains
`gpui`. Framework packages come first, with `xgpui-macros` before the core because
it is a required dependency. Child extensions follow the complete framework in
dependency order, at their existing `0.2.0` versions.

Generate the review plan without any registry access:

```sh
python3 scripts/prepare_registry_release.py --out /tmp/xgpui-release-plan
```

Add `--archives` to generate and inspect all archives using cached dependencies
and explicit offline Cargo commands. This is archive-content inspection, not
registry-backed build verification. The output must be a new directory outside
the repository. Reports record the source commit, dirty status, versions, phase
order, publication policy, development cycles and compatibility patches. Archive
runs also record checksums and package logs. Failed runs retain their partial
report; start a new output directory for another snapshot.

The manual-only **Prepare registry release (no registry access)** workflow runs
Python tests and uploads the plan. It has read-only repository permissions, no
registry credentials and no upload mode. It is not a publishing workflow.
At the offline preparation stage, extension `publish = false` flags remained intact.

Before any crates.io API/index/download access, registry-backed Cargo dry run,
credential setup, or upload, present the exact scope and commands to the project
owner and obtain explicit approval. Preparation does not authorize that next
phase. Name availability and ownership of `xgpui` have not been checked.

The user accepted existing Windows/Linux platform validation; local cross-check
limitations remain recorded as evidence limitations, rather than a request to
repeat that platform validation. Registry-equivalent dependency resolution,
Git fallback/development-cycle handling, resumable upload execution and
post-publication checks remain to be implemented after the approval boundary.

## gpui-luma conventions reviewed

The sibling `gpui-luma` repository's workspace/package manifests provide the
metadata pattern: Apache-2.0, project repository/homepage, focused descriptions,
keywords and Cargo categories. xgpui candidates use explicit fields because these
crates have independent Cargo workspaces. The project owner confirmed that all
release crates use Apache-2.0, the same license as Zed's GPUI. Existing upstream
attribution is preserved; bundled third-party assets retain their own notices.
Authors or registry ownership were not inferred.

The sibling `.github/workflows/release.yml` and `scripts/release.py` are useful
references for verification by default, explicit first-registration permission,
serialized/resumable publication, downloaded-content comparison and retained
diagnostics. Do not copy its single workspace version or macOS-only release job:
xgpui has separate framework/extension versions and issue #49 requires native
verification on Windows, Linux and macOS.

Recheck the official [Cargo publishing guide](https://doc.rust-lang.org/cargo/reference/publishing.html)
and [path/Git dependencies with registry versions](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#multiple-locations)
when implementing package verification and publication.

## Extension registry release 0.2.0

The project owner explicitly approved registry dry runs and publication of the six
extensions at 0.2.0. Their manifests now allow only `crates-io`. The nine framework
packages at 1.0.0 are already published. This approval does not authorize merging
the preparation branch or publishing another package or version.

`scripts/publish_extensions.py` fixes the scope to color, shader, svg, parley,
gamut and media, in that order. It stages a clean commit outside the checkout,
uses a fresh Cargo home without repository patches or credentials, and verifies
the four independent packages before the first upload. Gamut and media then
resolve the newly published color/shader versions for their own registry dry runs.
Every upload requires a successful dry run and archive inspection. Credentials
are passed only to upload subprocesses; logs redact the token.

For the explicitly approved publication, provide `CARGO_REGISTRY_TOKEN` through
the repository's organization secret and run:

```sh
python3 scripts/publish_extensions.py --out /tmp/xgpui-extensions-release \
  --publish --confirmation 'publish six extensions 0.2.0 only'
```

Without `--publish`, commands only verify packages; dependent dry runs require
their prerequisites to exist in the registry. A 429 retries only after the
registry's stated deadline, with at most three attempts per package. Other
failures stop. `--resume` requires the same source commit and original output;
published versions are skipped only after comparing their downloaded contents
and registry checksum with the verified archive. The report retains source SHA,
commands, logs, inspected file counts and checksums.

The [approved extension release run](https://github.com/gpui-luma-dev/xgpui/actions/runs/40798784916)
completed successfully on 2026-10-29 UTC from source commit
`6dd303959ab4d16e9669da96a985692afd67606e`. All six 0.2.0 registry dry runs passed,
and all six versions are published and unyanked. The downloaded registry archives
match the complete verified dry-run archive contents; checksums and packaged
source, README and license inputs were also independently checked.

Media encountered the new-crate rate limit once. The job waited until the
server's `2026-10-29 01:23:06 UTC` deadline, checked that the version was still
absent, and then published successfully. The workflow artifact retains the
report, logs and verified archives for this release.

A separate credential-free macOS consumer passed `cargo check` using exact
registry versions of all 15 packages, including the instanced gamut and portable
shader features. Cargo metadata confirmed every release dependency came from
crates.io, with no repository path patches. This checks native consumer compilation;
it does not claim execution of a GUI or cross-platform runtime tests.

## Apple bindings migration (#73)

The published versions above still resolve legacy `block 0.1.6` through Cocoa
and metal-rs on macOS and can emit `uninhabited_static` warnings. These published
packages do not include the local migration.

The local `codex/objc2-metal-interfaces` branch now uses objc2 and objc2-metal
throughout the Apple renderer, custom compositors, shader and gamut producers,
and the macOS windowing, input, menu, display and pasteboard paths. Native resource
owners use `Retained`; callbacks and dynamically registered AppKit subclasses use
the objc2 runtime and explicit native ABI types. The temporary `metal_compat`
bridge and legacy `block` patch are removed. The existing Core Foundation,
Core Graphics and Core Text C wrappers remain where they do not depend on the
legacy Objective-C runtime.

This is an unpublished local migration for native lab smoke testing. It changes
public native Metal resource types; downstream native producers must use the
objc2 types and encoder APIs. The sibling xgpui-lab ownership/upload probes have
been adapted locally. Existing pending test-support hooks are preserved.

Native Apple library, macOS, shader, gamut/instanced and color-math tests validate
the converted paths. The lab's native video/Surface and lifecycle/ownership
reports are additional local evidence, not proof of Intel Managed-storage behavior
or physical display calibration. Recheck source revisions, dependency graphs and
candidate consumer builds before making any release claim. No version changes,
registry publication or push are part of this work.
