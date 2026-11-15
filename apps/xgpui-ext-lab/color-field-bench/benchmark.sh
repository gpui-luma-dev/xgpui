#!/usr/bin/env bash
# Five recorded trials of both color-field methods into results/<run>.
# Run from the repository root after a release build:
#   just color-field-bench benchmark -- <run>
set -euo pipefail
run=${1:?usage: benchmark.sh <run-name>}
suite_dir="apps/xgpui-ext-lab/color-field-bench/results/$run"
mkdir -p apps/xgpui-ext-lab/color-field-bench/results
mkdir "$suite_dir"
rustc -Vv > "$suite_dir/compiler.txt"
git rev-parse HEAD > "$suite_dir/repository-commit.txt"
git status --short > "$suite_dir/working-tree.txt"
cp upstream.json "$suite_dir/upstream.json"
shasum -a 256 apps/xgpui-ext-lab/color-field-bench/src/*.rs apps/xgpui-ext-lab/Cargo.toml Cargo.lock .cargo/config.toml .cargo/validation.toml target/release/color-field-bench > "$suite_dir/source-hashes.txt"
if [[ "$(uname -s)" == Darwin ]]; then
    sysctl -n machdep.cpu.brand_string hw.memsize hw.model > "$suite_dir/hardware.txt"
    system_profiler SPDisplaysDataType > "$suite_dir/displays.txt"
    sw_vers > "$suite_dir/os.txt"
    pmset -g custom > "$suite_dir/power-settings.txt"
    pmset -g batt > "$suite_dir/power-source.txt"
fi
for trial in 1 2 3 4 5; do
    for method in layered sampled; do
        echo "$method trial $trial/5"
        report="$suite_dir/$method-$trial"
        if MTL_HUD_ENABLED=0 perl -e 'alarm 60; exec @ARGV or die $!;' target/release/color-field-bench --headless --method "$method" --animate --width 512 --height 384 --samples 128 --hue 210 --warmup 30 --frames 300 --json "$report.json" > "$report.log" 2>&1; then
            echo 0 > "$report.exit-status.txt"
        else
            status=$?
            echo "$status" > "$report.exit-status.txt"
            exit "$status"
        fi
    done
done
