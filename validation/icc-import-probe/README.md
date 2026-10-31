# Optional ICC engine investigation

This standalone validation package tests unmodified moxcms 0.8.1. It adds no
product dependency, file decoding, or production profile-import API.

```sh
cargo test --offline --locked --manifest-path validation/icc-import-probe/Cargo.toml
cargo run --offline --locked --manifest-path validation/icc-import-probe/Cargo.toml
```

Three tests cover corrected synthetic profile construction, 432 vectors against
an independent engine, and basic malformed/truncated-profile rejection. This
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

These results support keeping the engine as a candidate for this subset. They
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
