#!/usr/bin/env bash
# #284: build and run xgpui-luma's apps against this branch (GPUI 1.28.1 epic plus the
# #192 shell-color port). The luma temp branch patches every xgpui crate to the sibling
# ../xgpui checkout, which xgpui-test has already switched to this branch.
# Temporary: delete before the epic merges to main.

LUMA_BRANCH=temp/xgpui-1.28.1-test
LUMA_DIR=../xgpui-luma

note "xgpui-luma on $LUMA_BRANCH"
if [[ ! -d "$LUMA_DIR/.git" ]]; then
    git clone https://github.com/gpui-luma-dev/xgpui-luma.git "$LUMA_DIR" || { echo "clone failed" >&2; exit 1; }
fi
if [[ -n "$(git -C "$LUMA_DIR" status --porcelain)" ]]; then
    echo "$LUMA_DIR has uncommitted changes; commit or stash them, then rerun." >&2
    exit 1
fi
git -C "$LUMA_DIR" fetch origin "$LUMA_BRANCH" || exit 1
git -C "$LUMA_DIR" switch "$LUMA_BRANCH" 2>/dev/null \
    || git -C "$LUMA_DIR" switch -c "$LUMA_BRANCH" --track "origin/$LUMA_BRANCH" || exit 1
git -C "$LUMA_DIR" merge --ff-only "origin/$LUMA_BRANCH" || exit 1
{
    echo "xgpui: $(git rev-parse --abbrev-ref HEAD) @ $(git rev-parse --short HEAD)"
    echo "xgpui-luma: $LUMA_BRANCH @ $(git -C "$LUMA_DIR" rev-parse --short HEAD)"
} | report

note "Building the whole xgpui-luma workspace (first build takes several minutes)"
(cd "$LUMA_DIR" && cargo build --workspace) || { echo "xgpui-luma build failed" >&2; exit 1; }

note "xgpui-lab tests"
(cd "$LUMA_DIR" && cargo test -p xgpui-lab 2>&1 | grep -E '^test result|FAILED|panicked') | report

# Start a built luma app in the background; the runner's stop_example stops it.
run_luma() {
    local bin="$LUMA_DIR/target/debug/$1"
    [[ -f "$bin.exe" ]] && bin="$bin.exe"
    [[ -f "$bin" ]] || { echo "built app not found at $bin" >&2; return 1; }
    "$bin" &
    XGPUI_EXAMPLE_PIDS="${XGPUI_EXAMPLE_PIDS:-} $!"
    trap stop_example EXIT
    echo "Started $1 (pid $!)."
    sleep 3
}

run_luma xgpui-lab
step "Use the xgpui-lab window: switch looks and light/dark, open a few pages, then click another window and back." \
     "Title bar color follows the look and dims when inactive; pages render and respond; no crash."
stop_example

run_luma luma-studio
step "Use luma-studio: click through its controls and resize the window." \
     "Renders and redraws while resizing; controls respond; no crash."
stop_example

run_luma luma-radix-studio
step "Use luma-radix-studio: click through its controls and resize the window." \
     "Renders and redraws while resizing; controls respond; no crash."
stop_example

for shell in luma-shell-detached luma-shell-split-titlebar luma-shell-vscode luma-shell-2026; do
    run_luma "$shell"
    step "Use $shell: hover and click minimize/maximize/restore, drag the title bar, then close it with its close button." \
         "Controls draw and work, dragging moves the window, close closes it; no crash."
    stop_example
done

run_luma luma-neumorphic-demo
step "Use luma-neumorphic-demo: click its controls." "Soft shadows render; controls respond; no crash."
stop_example

run_luma luma-plot-viz
step "Use luma-plot-viz: pan or zoom a plot." "Plots render and update smoothly; no crash."
stop_example
