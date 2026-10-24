# API compatibility guidance

The labs are downstream consumers of the public GPUI API. They provide concrete
examples and regression checks as captured releases and local extensions evolve.
They do not establish full API stability, binary ABI compatibility, or identical
behavior across platforms.

Use [API_COVERAGE.md](API_COVERAGE.md) to find relevant evidence before changing
an API. Preserve existing consumer call patterns where practical. For an
intentional break, describe the old/new usage and migration, and update the
consumer deliberately. A rewritten test alone does not demonstrate compatibility.

Keep different kinds of evidence explicit:

- Compilation checks public call patterns and target/feature compatibility.
- Behavioral tests check named outcomes under the test environment.
- Native manual checks cover OS integration that mock windows cannot establish.
- GPU pixel checks and performance measurements support their particular
  workloads and backends; they are not general rendering or speed guarantees.

Add focused consumer assertions when changing behavior that existing labs do not
cover. Reuse the native application's implementation in tests where possible.
Keep new labs as standalone workspaces with their own lockfiles and focused CI;
the root build harness remains a separate captured-library validation target.

Compatibility comparisons should include the current development behavior and
the incoming upstream changes. Report checks actually performed and remaining
gaps, especially when only one backend or platform was exercised.
