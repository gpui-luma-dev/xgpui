---
name: xgpui-docs-sync
description: Check or update the xgpui documentation website after importing a GPUI release, including crate/API inventory, architecture, build/WASM recipes and the SVG scene model.
---

# Maintain xgpui docs after a release

Use this in the xgpui checkout containing `upstream.json`, `docs/roadmap` and
`tools/crate-roadmap`. The release is imported separately; this skill maintains
documentation against the imported local source. Do not trigger an import or
change the clean upstream branch as part of a docs refresh.

## Establish the comparison

Read repository instructions, `docs/roadmap/BUILDING.md`, `upstream.json` and
`docs/roadmap/release-review.json`. Inspect Git status before making changes.
Use the review record's `checkout_commit` as the prior documentation baseline;
read its `upstream.json` with `git show` and compare it with the current capture.
Use an explicitly supplied comparison ref instead when the user requests one.
Record the new release ref, source commit, package additions/removals/renames
and any downstream source changes relevant to the docs.

If the recorded commit is unavailable, try the available local Git history.
If no reliable baseline exists, audit the current capture and say that a
release-to-release comparison was unavailable. Do not invent an old release
or treat a missing baseline as evidence of no changes.

For **check** requests, inspect and regenerate ignored output, then report
stale content and required edits without modifying tracked docs. For **update**
requests, complete the relevant edits and validation below. A request to invoke
this skill without further qualification means check and update the docs.

## Inspect the source and maintain the right layer

Generated inventories are evidence, not a resolved public API or proof of
compatibility. Read the changed Rust modules and manifests directly. Compare
root re-exports, feature/target guards, macro support, public signature types
and platform implementations; do not infer reachability from `pub` alone.

| Documentation | What needs a human/agent source review |
| --- | --- |
| `notes.json` | One entry per captured package; role, surface summary, group, entrypoint and trimming hypotheses. Preserve exact package names. Investigate new/renamed crates rather than filling generic descriptions. |
| `guides.js` | Ownership and lifecycle claims: Application/App/Context, entities, notification and tracking, view caching, layout/prepaint/paint, input, scene presentation, text and async work. Follow changed implementations through their callers. |
| `build-guide.js` | Current feature forwarding/defaults, targets, local path patches, shader compilation, example paths, app snippets, browser constructor/lifecycle, fonts, WebGPU/WebGL fallback and threading. Existing validation statements are historical unless rerun for the new capture. |
| `pipeline.js` | Re-read `scene.rs`, `bounds_tree.rs`, the relevant Window painting/frame code and backend batch consumers. Check clipping/rejection, maximum-overlap order, explicit layers, enum tie order, per-type sorting, sprite tile/texture rules, replay and intermediate path rendering. Adapt the model and meaningful fixtures if Rust semantics changed. |
| `app.js`, `template.html`, `style.css` | Navigation, crate groups/order, labels, source-link behavior and readable controls. Keep Core → Platforms → Data → Transport, with core gpui before gpui-macros unless the user changes that preference. Preserve the footer attribution. |
| `tools/crate-roadmap` | Fix traversal/serialization only when the new source requires it. Preserve cfg/re-export/macro limitations and warnings. Do not silently omit unparsed modules or expand the inventory's claims. |

Search the narrative and fixtures for old versions, crate counts, package names,
paths, symbol names and embedded line references. Generated feature tables and
manifest excerpts refresh automatically; their prose interpretation does not.
Inspect the Rust around every affected source link: a surviving line anchor
does not prove it still points at the intended implementation.

Keep trimming ideas labeled as hypotheses. Keep the SVG labeled as a teaching
model, not a GPUI execution trace. Passing JavaScript fixtures does not establish
that the model still matches a new Rust implementation.

## Regenerate and validate

Run from the repository root:

```sh
cargo run --locked --manifest-path tools/crate-roadmap/Cargo.toml
node docs/roadmap/verify.mjs
```

The generator reads only the package map, not the duplicated transformed staging
tree. It rejects missing/extra crate notes. Fix those notes from source before
retrying. Generated `docs/roadmap/site/` is ignored; do not hand-edit or commit it.
Remove obsolete generated source pages if modules/packages disappear, then
regenerate. Do not use a broad repository cleanup command.

Inspect generator warnings, package coverage, source links and the review diff.
For generator changes, run:

```sh
cargo fmt --manifest-path tools/crate-roadmap/Cargo.toml --check
cargo test --locked --manifest-path tools/crate-roadmap/Cargo.toml
cargo clippy --locked --manifest-path tools/crate-roadmap/Cargo.toml --all-targets -- -D warnings
```

When build recipes, signatures or feature graphs change, check the affected
documented app snippets in a temporary workspace against this checkout's local
patches and compatibility config. Native `cargo check` and WASM `cargo check`
are separate checks; neither proves browser runtime behavior or a linked package.
Do not claim threaded WASM, worker packaging, Windows/Linux or mobile support
from an unrelated host build. If a command is blocked by a missing toolchain,
SDK, dependency or network access, record that limit and finish other checks.

Use Rust/Node tooling; do not introduce or run Python for this workflow.
Do not launch GUI applications or browsers without explicit user approval.
Do not edit README.md, captured GPUI code, release import automation or generated
root build manifests merely to make the docs checks pass.

## Record and hand off

For update requests, update `docs/roadmap/release-review.json` only after reviewing
the relevant narrative, model and build claims, not merely after generation.
Keep its schema version, record the inspected capture/checkout, and describe
which checks actually ran. The checkout commit may identify the imported-source
state before the documentation commit. Mark skipped/unavailable checks explicitly;
do not carry a prior release's passing validation forward as a new result.
If unresolved facts remain, record the review as partial with those gaps.

Report the compared releases, meaningful architecture/API/build changes, docs
updated, validation performed and remaining gaps. Link the generated local site.
Publishing, pushing and creating/updating a PR require authorization from the
current user session; invoking this skill alone does not authorize them. Never
merge or close a PR without explicit approval.
