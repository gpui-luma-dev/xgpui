# Issue 37: rounded overflow clipping

[Issue #37](https://github.com/gpui-luma-dev/xgpui/issues/37) investigates clipping
actual descendant content to rounded container boundaries.

## Investigation baseline

- [Source-grounded feasibility proposal](feasibility.md): source trace, architecture
  alternatives, compatibility inventory, observed results and integration gates.
- [Runnable probe and reproduction instructions](../../apps/xgpui-ext-lab/clip-chain-probe/README.md).
- [Captured results](../../apps/xgpui-ext-lab/clip-chain-probe/results/), including the
  [demo screenshot](../../apps/xgpui-ext-lab/clip-chain-probe/results/demo.jpg).

The isolated probe works on the tested native Metal, wgpu and WebGL2 paths. It is
not an integrated GPUI implementation or a production performance/edge-quality claim.
The baseline introduced no core public API changes. The implementation milestone
adds experimental hooks pending the [core API review](api-review.md).

## Agreed next implementation scope

Focus on rounded UI containers with configurable backgrounds, gradients and radii,
whose descendant backgrounds otherwise show square corners outside the border.
Compare existing `overflow_hidden()` with experimental rounded descendant clipping
in a native lab. Preserve legacy overflow behavior and investigate nested rounded
clips, matching input geometry, retained replay and backend support.

Ellipses remain feasibility groundwork; polygons and arbitrary paths are deferred.
Keep work on issue-37 branches, with PRs targeting `issue-37/epic`. Nothing goes into
`main`, and no PR is merged without explicit user approval. Concrete core public API
changes require individual final review under the repository instructions.

## Native implementation milestone

- [Milestone report](rounded-overflow-milestone.md).
- [Native rounded-overflow lab](../../apps/xgpui-ext-lab/rounded-overflow-lab/README.md): `just rounded-overflow`.
- [Core public API review](api-review.md), pending final approval.
