# Issue #271: GNOME Wayland windows stop presenting after their second frame.
# Run on the Fedora laptop in a GNOME Wayland session (and on the new Linux box
# if it runs a Wayland desktop). Each window runs about 40 seconds; the script
# checks frame counts itself, saves full frame-loop traces next to this log and
# posts its digests and your answers to #271 when it finishes.
#
# It also records versions and Vulkan surface color spaces, for #146.

case "$(uname -s)" in
    Linux) ;;
    *) note "This test is for Linux Wayland sessions; nothing to do on $(uname -s)."; exit 0 ;;
esac

if [[ "${XDG_SESSION_TYPE:-}" != wayland || -z "${WAYLAND_DISPLAY:-}" ]]; then
    note "Not a Wayland session (XDG_SESSION_TYPE=${XDG_SESSION_TYPE:-unset}, WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-unset})."
    note "Log out and choose the Wayland session at the login screen, then run again."
    exit 1
fi

# Runners from before result posting reached main don't post; borrow this
# branch's copy of the runner's helpers and post from this script instead.
if [[ -z "${XGPUI_TEST_POSTS:-}" ]]; then
    # shellcheck source=scripts/remote-test.sh
    source scripts/remote-test.sh
    set +u +o pipefail
    XGPUI_TEST_REPORT=$(mktemp "${TMPDIR:-/tmp}/xgpui-report.XXXXXX")
    XGPUI_TEST_POST_TO=$(mktemp "${TMPDIR:-/tmp}/xgpui-post-to.XXXXXX")
    export XGPUI_TEST_REPORT XGPUI_TEST_POST_TO
    self_post=1
fi
post_to 271

out_base=${XGPUI_TEST_LOG:-$HOME/xgpui-test-logs/271-$(date +%Y%m%d-%H%M%S).log}
out_base=${out_base%.log}
mkdir -p "$(dirname "$out_base")"

# ---------------------------------------------------------------- system info
note "System and graphics stack"
{
    echo "kernel:   $(uname -r)"
    echo "session:  ${XDG_SESSION_TYPE} desktop=${XDG_CURRENT_DESKTOP:-unset} display=${WAYLAND_DISPLAY}"
    [[ -r /etc/os-release ]] && echo "os:       $(. /etc/os-release && echo "$PRETTY_NAME")"
    command -v gnome-shell >/dev/null && echo "gnome:    $(gnome-shell --version 2>/dev/null)"
    command -v plasmashell >/dev/null && echo "plasma:   $(plasmashell --version 2>/dev/null)"
    if command -v rpm >/dev/null; then
        for pkg in mutter kwin mesa-vulkan-drivers mesa-dri-drivers vulkan-loader; do
            rpm -q "$pkg" 2>/dev/null | grep -v 'not installed' | sed 's/^/package:  /'
        done
    elif command -v dpkg-query >/dev/null; then
        dpkg-query -W -f='package:  ${Package} ${Version}\n' \
            mutter kwin-wayland mesa-vulkan-drivers libvulkan1 2>/dev/null
    fi
    command -v lspci >/dev/null && lspci | grep -Ei 'vga|3d|display' | sed 's/^/gpu:      /'
    # Displays and their scale, as GNOME reports them (no-op elsewhere).
    command -v gdctl >/dev/null && gdctl show 2>/dev/null | sed 's/^/gdctl:    /'
} 2>&1 | report

vulkan_log="$out_base-vulkaninfo.txt"
if command -v vulkaninfo >/dev/null; then
    note "Vulkan devices and surface color spaces (#146)"
    vulkaninfo --summary 2>&1 | grep -E 'deviceName|driverName|driverInfo|apiVersion' | sed 's/^ */vulkan:   /' | report
    # The full report lists each presentable surface's formats and color spaces.
    timeout 60 vulkaninfo >"$vulkan_log" 2>&1
    {
        echo "surface color spaces offered (count of format entries):"
        grep -oE 'VK_COLOR_SPACE_[A-Z0-9_]+' "$vulkan_log" | sort | uniq -c | sed 's/^/    /'
    } | report
    echo "full report: $vulkan_log"
else
    note "vulkaninfo is not installed (Fedora package vulkan-tools); skipping the color-space report."
fi

# ---------------------------------------------------------------- build
note "Building the animation example and the shell lab (first build can take several minutes)"
cargo build -p xgpui --example animation || { echo "build failed" >&2; exit 1; }
cargo build --locked --config .cargo/validation.toml --target-dir target \
    --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin shell-lab ||
    { echo "build failed" >&2; exit 1; }
animation_bin=${CARGO_TARGET_DIR:-target}/debug/examples/animation
shell_bin=target/debug/shell-lab

export RUST_LOG="info,xgpui::wayland::frame=trace,xgpui::present=debug,wgpu_hal=warn,wgpu_core=warn"

presents() {
    grep -oE 'presented #[0-9]+' "$1" | tail -1 | grep -oE '[0-9]+' || echo 0
}

# start_traced NAME BIN [ARGS...]: start a window with its log in NAME's trace file.
start_traced() {
    local name=$1 bin=$2
    shift 2
    trace="$out_base-$name.trace.log"
    "$bin" "$@" >"$trace" 2>&1 &
    XGPUI_EXAMPLE_PIDS="${XGPUI_EXAMPLE_PIDS:-} $!"
    trap stop_example EXIT
    echo "Started $name (pid $!), trace: $trace"
}

# digest NAME: summarize one trace for pasting back into #271.
digest() {
    local name=$1 trace="$out_base-$1.trace.log"
    echo "--- digest: $name"
    grep -m1 'request present mode' "$trace" | sed 's/^.*\] /    /'
    grep -m1 'watchdog enabled' "$trace" | sed 's/^.*\] /    /'
    echo "    presents: $(presents "$trace")"
    echo "    callback stalls: $(grep -c 'outstanding for' "$trace")"
    echo "    late callbacks: $(grep -c 'arrived after' "$trace")"
    echo "    failed presentations: $(grep -c 'presentation failed' "$trace")"
    echo "    slow acquires: $(grep -c 'acquire took' "$trace")"
    grep -oE 'surface (acquire )?(Timeout|Occluded|Lost|Outdated|suboptimal)' "$trace" |
        sort | uniq -c | sed 's/^/    acquire outcome: /'
    grep -Ei 'panic|error' "$trace" | grep -v 'frame callback' | head -5 | sed 's/^/    error: /'
    if grep -q 'outstanding for' "$trace"; then
        echo "    first stall, with the 25 frame-loop lines before it:"
        grep -m1 -B25 'outstanding for' "$trace" | sed 's/^/      /'
    elif (( $(presents "$trace") < 60 )); then
        echo "    few presents and no stall warning; last 25 lines:"
        tail -25 "$trace" | sed 's/^/      /'
    fi
}

# check_running NAME SECONDS: wait, then report whether presents kept increasing.
check_running() {
    local name=$1 seconds=$2 trace="$out_base-$1.trace.log" before after
    before=$(presents "$trace")
    sleep "$seconds"
    after=$(presents "$trace")
    echo "auto-check $name: presents $before -> $after over ${seconds}s"
    if (( after - before < seconds * 10 )); then
        echo "auto-check $name: STALLED (expected roughly 60 per second while animating)"
    else
        echo "auto-check $name: drawing continuously"
    fi
}

# run_variant NAME DESCRIPTION [ENV=VALUE...]: one animation window under a configuration.
run_variant() {
    local name=$1 description=$2
    shift 2
    note "Animation window: $description"
    start_traced "$name" env "$@" "$animation_bin"
    sleep 3
    check_running "$name" 5 | report
    step "Watch the arrow circle under 'Hello Animation' for about 5 seconds. Only the arrow moves on its own; the blue dot and colored bar move only when you click the grey box." \
         "The arrow keeps turning back and forth without stopping. If it froze after the first frame or two, answer fail."
    step "Resize the window by dragging its corner, then click another window and click back." \
         "The window redraws at each new size and keeps animating after the focus changes."
    note "Leave the window alone for 15 seconds (the script waits)."
    check_running "$name" 15 | report
    stop_example
    sleep 1
    digest "$name" | report
}

# ---------------------------------------------------------------- runs
run_variant mailbox "default configuration (Mailbox present mode)"
run_variant fifo "FIFO present mode (XGPUI_WAYLAND_PRESENT_MODE=fifo)" \
    XGPUI_WAYLAND_PRESENT_MODE=fifo
run_variant watchdog "Mailbox with the frame-callback watchdog (XGPUI_WAYLAND_FRAME_WATCHDOG=1)" \
    XGPUI_WAYLAND_FRAME_WATCHDOG=1

note "Shell lab with client-side decorations (the window from #149)"
start_traced shell "$shell_bin" --client-decorations
sleep 3
step "Hover over the shell lab's title-bar buttons, then scroll the lab's content up and down." \
     "Hover highlights appear and the content scrolls. If nothing reacts on screen, answer fail."
step "Resize the shell lab window, then maximize and restore it with its title-bar button." \
     "It redraws at each size and after maximize/restore."
stop_example
digest shell | report

note "Full traces are next to this log: $out_base-*.trace.log"

if [[ -n "${self_post:-}" ]]; then
    post_results "$(git branch --show-current)" "$(git rev-parse --short HEAD)" 0 \
        "$XGPUI_TEST_RESULTS" "$XGPUI_TEST_REPORT" "$XGPUI_TEST_POST_TO" "" "$out_base.log"
    rm -f "$XGPUI_TEST_REPORT" "$XGPUI_TEST_POST_TO"
fi
