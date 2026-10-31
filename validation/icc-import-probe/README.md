# Optional ICC engine investigation

This standalone validation package tests unmodified moxcms 0.8.1. It adds no
product dependency, file decoding, or production profile-import API.

```sh
cargo test --offline --locked --manifest-path validation/icc-import-probe/Cargo.toml
cargo run --offline --locked --manifest-path validation/icc-import-probe/Cargo.toml
```

The original three checks cover corrected synthetic profile construction, 432
gamma-profile vectors against an independent engine, and basic malformed/truncated-profile rejection.
The curve extension below adds diagnostics and two unresolved acceptance gates. This
small malformed-input check is not a parser security or resource-budget audit.

## Findings

Our original probe changed Display P3's curves to linear while leaving its sRGB
CICP tag attached. The extended path consulted that stale tag. Clearing the tag
on this deliberately modified synthetic destination makes its metadata consistent.
The original probe also incorrectly expected Adobe green's P3 blue to be negative;
its red is negative and blue positive. Neither problem required a dependency patch.
Do not strip metadata from real profiles as a workaround: conflicting metadata
requires an explicit import policy.

The floating transform has float32 internals even with float64 input/output.
The comparison budget is 16 f32 epsilon times decoded source-channel energy,
allowing matrix cross-channel error near cancellation while remaining tight near
black. Black must be exactly zero. Observed maximum absolute error was
1.1920928955078125e-7; maximum budget usage was 6.49% on this Mac.

The original gamma-only results support that tested subset. The broader curve
precision gates below currently prevent adopting the general transform path. They
do not establish all ICC curves/profile classes, alpha semantics, memory bounds,
throughput, platform parity, or native monitor output. No production selection
or maintained fork follows from these tests.

## Fixture provenance

`generate.py` uses the system Little CMS **2.19** (encoded version 2190) through
ctypes, independently of moxcms. It creates RGB profiles from declared primary
chromaticities and D65 white:

- Adobe RGB: (0.64,0.33), (0.21,0.71), (0.15,0.06), gamma 563/256.
- Linear P3: (0.68,0.32), (0.265,0.69), (0.15,0.06), gamma 1.
- Linear Rec.2020: (0.708,0.292), (0.170,0.797), (0.131,0.046), gamma 1.

These are newly generated synthetic fixtures, not Adobe or monitor-vendor profile
files. The generator normalizes the creation timestamp and profile ID, reopens
the serialized profiles, and evaluates relative-colorimetric RGB-double transforms
with optimization and one-pixel caching disabled. The references therefore include
serialized ICC quantization. Six channel levels form 216 inputs per destination.
No renderer or moxcms code produces the expected values.

To regenerate or check on a machine with Little CMS 2.19 installed:

```sh
python3 validation/icc-import-probe/generate.py
python3 validation/icc-import-probe/generate.py --check
```

Other Little CMS versions/platforms may produce different profile metadata or
floating-point references; review those differences rather than replacing fixtures
without checking them. Ordinary Rust tests use the committed fixtures. The
standalone Cargo.lock pins its validation dependencies separately from the product.

Remaining work before an optional adapter: real-profile fixtures, independent
sampled/parametric curves, coherent PCS/destination construction, metadata precedence,
profile/table limits, cancellation, alpha handoff and worker admission. File-format
support stays in application adapters.

## Curve precision gates — unresolved

The extended probe adds a serialized five-parameter sRGB-style curve and three
independent 257-entry sampled curves (red gamma 2, green 2.2, blue 1.8), all with
Adobe RGB primaries. It checks 1,032 additional vectors against both Little CMS
and an independent 120-digit calculation from the actual serialized matrix and
curve tags. Inputs include near-black ramps and f32 neighbors on both sides of
parametric thresholds and selected sampled knots.

The direct oracle evaluates quantized parametric coefficients exactly and uses
continuous piecewise-linear interpolation between normalized u16 sampled entries.
It does not substitute ideal sRGB coefficients for serialized values. The tiny
fixture-only tag reader in `generate.py` handles only these known generated tags;
it is not a production ICC parser.

| Source curves | Vectors | Outside decoded-energy f32 budget | Maximum absolute error vs direct oracle |
| --- | ---: | ---: | ---: |
| Parametric | 510 | 298 | 1.04635e-6 |
| Independent sampled | 522 | 386 | 8.75098e-6 |

These counts are for moxcms 0.8.1 with the existing options on this Mac. Near-black
relative errors can be much larger than these absolute errors suggest. For
example, sampled blue input 0.0001 produces P3 blue 1.018837338e-6 instead of the
continuous-interpolation reference 1.112825128e-6 (about 8.45% low). Parametric
blue input 0.0001 differs by about 0.0088% in that channel. This is a mismatch with
our chosen numerical contract, not a blanket claim that the library is defective.

Little CMS also differs from the direct sampled-curve oracle (maximum absolute
error 1.75589e-5 here). Its sampled-curve numerical behavior cannot serve as the
sole definition of the continuous interpolation contract. The direct oracle and
second engine are separate evidence, not interchangeable sources of truth.

Ordinary tests now report **4 passed, 2 ignored**. The two ignored tests are strict
acceptance gates, not accepted support. Their f32 budgets were not widened:

```sh
# Diagnostic report; prints every category's precision failures.
cargo run --offline --locked --manifest-path validation/icc-import-probe/Cargo.toml
# Both commands intentionally fail while these precision gates remain unresolved.
cargo run --offline --locked --manifest-path validation/icc-import-probe/Cargo.toml -- --strict-curves
cargo test --offline --locked --manifest-path validation/icc-import-probe/Cargo.toml -- --ignored
```

Recommendation: do not adopt this transform path for general RGB matrix/TRC
profiles under the current accuracy contract. Evaluate a parsing-only adapter
that passes validated colorants and curves into our color core. That requires
adding precise sampled/parametric evaluation to the core and validating parser
limits and profile semantics. This avoids a dependency fork without committing
to owning a general ICC reader. No product dependency or behavior changed here.
