# Paint GPU validation

The GPU workflows validate rendering at the checked-out commit. Rust **1.99.0**,
`.cargo/validation.toml`, and committed standalone lockfiles define the build.
All Cargo commands use `--locked`. The backend lockfiles were seeded from
`apps/xgpui-ext-lab/paint-lab/Cargo.lock` to retain compatible shared dependency versions.
When updating dependencies, refresh affected locks using the same validation
configuration and pinned compiler; commit the locks with the manifests.

## Dispatch a fix

Push the fix to a branch, then select that branch in Actions → Run workflow, or:

```sh
gh workflow run paints-windows-native.yml --ref YOUR_BRANCH -f scope=full
gh workflow run paints-windows-wgpu.yml --ref YOUR_BRANCH -f scope=full
gh workflow run paints-linux-vulkan.yml --ref YOUR_BRANCH -f scope=full
```

Each command runs only the selected backend. The workflow definitions must be
available on the default branch for GitHub's dispatch UI/CLI discovery. To run
full acceptance across all three backends at one commit:

```sh
gh workflow run portable-paints.yml --ref YOUR_BRANCH
```

Check the SHA in each job name, summary, and `timings.json`. Rerunning an existing
Actions run tests its **original SHA**, even after a new fix is pushed. Dispatch a
new run on the updated branch to test the new fix. PR-triggered jobs validate the
PR merge commit; manually dispatched jobs validate the selected branch tip.
Full cross-platform acceptance requires successful **full** results for all
three backends at the same SHA.

## Focused diagnostics

`scope=full` is the default. Other scopes are explicitly diagnostic and always
report `acceptance: false`, even when successful:

| Scope | Windows native | Windows wgpu / Linux Vulkan |
| --- | --- | --- |
| `color-gamut` | Wide SDR recording/composition, presentation policy, color/shader math, gamut producers and instanced adapters | Unavailable; select native |
| `producer-cache` | Exact coverage lookup/allocation and D3D11 WARP preparation/recovery | Exact coverage lookup/allocation, wgpu preparation/recovery, compositor uniform growth/recovery |
| `paint-parity` | Native application check, complete paint-lab native/wgpu parity suite, mesh capture | Portable paint references and low-alpha saturated edges |
| `full` | All original native checks and capture | All original backend checks |

For example:

```sh
gh workflow run paints-windows-native.yml --ref YOUR_BRANCH -f scope=color-gamut
gh workflow run paints-linux-vulkan.yml --ref YOUR_BRANCH -f scope=producer-cache
```

All backends additionally run the main-pass brush library tests in full/parity scope.
The native Windows job enables D3D11/WARP brush parity alongside wgpu.

`.github/paints/commands.json` is the single acceptance command inventory;
`.github/paints/run.py` selects scopes and runs it. Before each test, it executes
the same Cargo command with `--no-run` to measure compilation separately. Capture
similarly runs `cargo build` before the original `cargo run`. Test/capture time
includes Cargo's warm fingerprint check and process startup. No test command or
native capture from the original workflow was removed.

## Builds, cache, and evidence

Every manifest in a job uses the absolute `CARGO_TARGET_DIR` pointing to
`target/paint-validation`. Cargo can reuse compatible dependencies there; feature,
profile, target, compiler and crate version differences still cause builds.
Incremental compilation is disabled for predictable CI caching. Runner images
are pinned to Ubuntu 24.04 and Windows 2025; Rust is pinned independently.

Cache keys isolate runner OS/architecture, backend, full compiler identity and
validation configuration/runner implementation. Manifest and lockfile hashes
prefer an exact dependency match; a compatible fallback can restore older
builds, which Cargo rechecks. Unique run/attempt suffixes allow completed new
compilation to be saved even after test failures. Cancellation skips saving.
The cache contains registry/git dependencies and target outputs. It contains no
result logs or native capture. A restored cache is never validation evidence;
all selected tests execute again.

Each run uploads `paints-BACKEND-SCOPE-SHA` with per-command build/test/capture
logs, a summary and `timings.json`. Native full/parity runs also retain the
`native-dx11-mesh` PNG artifact. Expand the command groups in the Actions job to
inspect live output. Setup failures appear in the corresponding Actions step;
command failures retain the timings and logs collected up to that point.

## Cold/warm measurement protocol

Use the same SHA, backend, scope, pinned compiler and runner image for both runs:

```sh
gh workflow run paints-linux-vulkan.yml --ref YOUR_BRANCH -f scope=full -f cache=cold
# Wait for completion and cache save before dispatching the warm run.
gh workflow run paints-linux-vulkan.yml --ref YOUR_BRANCH -f scope=full -f cache=warm
```

Repeat for both Windows backends. Cold mode skips restoration and still saves
completed compilation; warm mode restores it. Check the recorded restored cache
key and compiler identity before comparing. Use a quiet branch to avoid other
runs replacing the most recent matching cache. GitHub cache ref visibility still
applies, so compare runs on the same branch/event type.

Download both timing artifacts. Compare `setup_seconds`, sums of `results.seconds`
by `build`, `test` and `capture`, and the `Compiling` lines in build logs. Setup
covers toolchain/dependencies/cache restoration after checkout; build includes
Cargo dependency fetching. Compare per-manifest first builds as well as later
commands to distinguish restored compilation from within-job reuse. Record run
URLs, SHA, cache keys, runner image versions (Actions setup logs), phase totals
and compilation counts. Repeated warm tests must still pass. Hosted cold/warm
numbers and GPU correctness must be established by these actual CI runs; local
orchestration tests do not demonstrate a performance improvement.

## Triggers and cancellation

Each backend has PR path triggers for Cargo configuration/manifests/locks, GPUI
core/platform/shader sources, extension crates, paint-lab, and validation
orchestration. Shared core/shader changes intentionally run all three backends.
Documentation-only changes skip automatic GPU work; dispatch remains available.
The full-platform workflow is manual to avoid duplicating the three PR jobs.

Concurrency groups contain PR number or branch ref, backend, scope and cache
mode. A new run supersedes an older matching run, including one started through
the full entry point. Other backends, focused scopes, PRs/branches, and cold/warm
measurement modes remain independent. Avoid manually starting a duplicate full
run while waiting for the same backend/scope on the same branch.

To check orchestration locally without a GPU:

```sh
python3 -m unittest discover -s .github/paints -p 'test_*.py'
python3 .github/paints/run.py --backend windows-native --scope full --dry-run
```
