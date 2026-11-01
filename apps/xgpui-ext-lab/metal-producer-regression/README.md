# WideColor regression — issue #110

Standalone headless macOS app exercising the migrated `WideColor` through the
portable content executor and the real window compositor. No window navigation
is needed. Run from the repository root:

```sh
MTL_DEBUG_LAYER=1 cargo run --manifest-path apps/xgpui-ext-lab/Cargo.toml \
  --bin metal-producer-regression --features runtime-shaders
```

The supervisor runs three children, each with a 15-second wall-clock deadline:

1. A single-producer control.
2. The original 512-draw red/green scene. Equal solid recipes may share one output.
3. A 512-draw stress scene with distinct natural source extents. All output pixels
   stay exactly red/green, but each draw requires its own native image/operation.
   This prevents deduplication alone from hiding the original submission bug.

Each child replaces dense → sparse → dense for three cycles, replays each scene
for three frames, and checks every final pixel against exact opaque linear-P3
FP16 words. Producer completion is awaited outside UI drawing with a deadline;
strict frame errors fail the child. Only the final compositor result is read back.
Native producer work never uses CPU pixel readback. Replay must use cached images
without submitting producer work again.

What to look for: every child finishes; each frame reports `readback verified`,
`producer_readbacks=0`, and `cached_replay=true`. Original repeated-color draws
report one producer operation; distinct dense draws report 512. `max_in_flight`
stays at or below 2. The final line starts **PASS**, and the process exits 0.
A timeout, pixel mismatch, typed resource error, or unexpected resubmission fails.
The parent kills/reaps a stalled child; compilation precedes the timeout.

For a larger independent-output run:

```sh
MTL_DEBUG_LAYER=1 cargo run --manifest-path apps/xgpui-ext-lab/Cargo.toml \
  --bin metal-producer-regression --features runtime-shaders -- \
  --distinct --producers 1024 --frames 3 --cycles 3 --timeout-seconds 30
```

The shared portable-content GPU budget is bounded. Excessive distinct-source
counts may return a resource-limit error; that is a failure of this strict run,
not permission to report stale pixels as success. The old native `WideColor`
Metal/D3D11 preparation code has been removed. Other legacy producer types remain
separate migration work, and this app does not close issue #110 automatically.
