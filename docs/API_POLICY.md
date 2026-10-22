# API compatibility policy

## Purpose

xgpui follows captured GPUI releases while preparing narrowly scoped API changes
for custom rendering and wide-gamut image applications. Exercise applications
are consumer contracts: they show which existing call patterns and observable
behaviors a change must preserve, or deliberately migrate.

This is an incremental compatibility lab, not a claim that the whole GPUI API
is stable or covered. The coverage map records evidence and gaps.

## What an exercise protects

- **Source compatibility:** existing public API calls continue compiling against
  the development branch. Examples must use public GPUI APIs and real application
  code paths; they must not access private renderer or platform internals.
- **Behavior:** headless assertions cover observable outcomes, including callback
  delivery, cancellation, ownership, and window lifetime. Building is not proof
  that those behaviors remain correct.
- **Native integration:** manual checks cover the real window manager, menus,
  display behavior, and application exit. Headless tests do not establish them.
- No binary ABI, cross-platform equivalence, rendering accuracy, or performance
  guarantee is implied without corresponding evidence.

## Changing an API

1. Identify affected capability rows in [API_COVERAGE.md](API_COVERAGE.md).
2. Preserve the current exercise call pattern and expected results when feasible.
   Prefer additive extension points and explicit capabilities over changing
   existing defaults for experimental rendering features.
3. Add a focused exercise and behavioral assertion for the new capability.
4. For an intentional break, document the old and new usage, behavioral impact,
   migration steps, and which existing exercise must change. Do not silently
   rewrite consumers just to make a breaking change appear compatible.
5. Compile the workspace and run affected headless tests. Record manual checks
   actually performed, plus platform and performance coverage still missing.

Compatibility is assessed against the parent development revision, not just the
original upstream snapshot. The clean upstream branch remains the import source;
downstream changes and their consumer evidence belong on development branches.

## Adding an exercise

Each exercise must have a bounded purpose, named public API families, expected
outcomes, automated evidence, manual steps, and explicit non-goals. Register it
in the coverage map and CI. Separate the small native entry point from reusable
exercise behavior so headless tests can call the same implementation.

New GUI applications are compiled by automation. Launch them only with explicit
user authorization. Do not turn screenshots or manual checklists into claims of
verified behavior unless those checks were performed.
