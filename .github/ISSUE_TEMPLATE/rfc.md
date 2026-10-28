---
name: RFC
about: Propose a substantial feature or architectural change
title: 'RFC: '
labels: ''
assignees: ''
---

<!-- Based on https://github.com/gpui-luma-dev/xgpui/issues/57.
Scale the detail to the proposal. Remove sections that do not apply.
Distinguish existing capabilities from proposed work and measured results from targets. -->

## Summary

What are you proposing, who benefits and what problem does it solve?
Identify affected crates, backends and applications.

## Goals and scope

Describe the intended behavior and explicitly deferred work. Include a concrete
usage example or proposed API where useful.

## Architectural pillars

Explain the main components, data flow and ownership. Which existing XGPUI
capabilities can be reused, and which APIs or dependencies need to change?
Describe native and browser behavior, including fallbacks where relevant.

## Feasibility and engineering lift

| Component | Estimated effort | Existing foundation and required work |
| --- | --- | --- |
| | | |

Call out assumptions, dependencies and work that needs a prototype before the
design can be considered feasible.

## Phased milestone roadmap

Describe small, reviewable milestones. For each, identify the deliverable,
prerequisites and observable acceptance criteria. Separate the initial useful
release from later enhancements.

## Architectural contracts and resource bounds

Describe relevant invariants: memory and cache limits, worker admission and
cancellation, GPU resource lifetime, steady-state rendering costs, source/data
fidelity, color and alpha domains, accessibility and reduced-motion behavior.
Explain failure handling and compatibility implications.

## Verification and acceptance plan

- **Automated tests:** correctness, edge cases, invariants and resource limits.
- **Rendering checks:** deterministic captures and relevant native/browser backends.
- **Performance:** representative workloads, metrics and proposed thresholds.
- **Interactive checks:** lab/demo recipes, accessibility and user-visible behavior.

Specify which platforms must pass for each milestone. Keep compilation evidence
separate from runtime verification, and identify any remaining validation gaps.
Use targets under `apps/xgpui-ext-lab/` for proposed extension labs and demos.

## Alternatives and tradeoffs

What other approaches were considered? Explain the costs, limitations and why
this direction is preferred, including the option of leaving things as they are.

## Open questions

List unresolved decisions and the feedback or evidence needed to resolve them.
Link related issues, guides, prototypes or measurements.
