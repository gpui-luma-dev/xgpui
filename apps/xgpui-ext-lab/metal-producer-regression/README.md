# Metal producer regression — issue #110

Standalone headless macOS app exercising the unchanged native renderer with
independent `WideColor` texture producers. Every cell is inside the viewport.
No window or app navigation is needed.

From the repository root:

```sh
cargo run --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin metal-producer-regression
```

The parent first runs a single-producer control, then a dense scene in a separate
child process. Each child has a 15-second wall-clock limit; the parent kills and
reaps a stalled child and exits unsuccessfully. Compilation happens before this
timeout starts. Preparation begin/end markers identify where progress stopped.

The default stress scene has 512 producers. Each child replays scenes for three
frames and replaces dense → sparse → dense scenes for three cycles. Every frame
is read back and checked against exact opaque red/green linear-P3 FP16 pixels.

```sh
cargo run --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin metal-producer-regression -- \
  --producers 1024 --frames 3 --cycles 3 --timeout-seconds 30
```

Expected on the affected renderer: the control passes, and sufficiently dense
preparation stalls before submission; the parent reports a timeout and exits 1.
After a framework fix, both children must finish with verified readback and exit 0.
A timeout is a failure signal, not proof of queue exhaustion by itself: inspect
the last markers or sample the child to distinguish startup, encoding, and GPU
readback stalls.

This app uses the renderer's normal queue. Its capacity is neither overridden nor
assumed; increase `--producers` if a particular machine does not reproduce. It
does not test cached command identities, injected failures, or worker uploads.
