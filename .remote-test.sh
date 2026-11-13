# Issue #194: window-control presenter hook (app-supplied SVG artwork).
# Run on Fedora (GNOME Wayland) and on Windows. The shell draws window controls
# itself on Linux client frames and on Windows app-composed title bars.

start_shell_lab() {
    note "Building the shell lab (first build on a branch can take several minutes)"
    cargo build --locked --config .cargo/validation.toml --target-dir target \
        --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin shell-lab ||
        { echo "build failed" >&2; return 1; }
    local bin=target/debug/shell-lab
    [[ -f "$bin.exe" ]] && bin="$bin.exe"
    "$bin" "$@" &
    XGPUI_EXAMPLE_PIDS="${XGPUI_EXAMPLE_PIDS:-} $!"
    trap stop_example EXIT
    echo "Started shell-lab (pid $!)."
    sleep 2
}

# An older runner copy does not post results; borrow this branch's runner
# helpers and post from this script instead (checking gh before testing).
if [[ -z "${XGPUI_TEST_POSTS:-}" ]]; then
    # shellcheck source=scripts/remote-test.sh
    source scripts/remote-test.sh
    set +u +o pipefail
    XGPUI_TEST_REPORT=$(mktemp "${TMPDIR:-/tmp}/xgpui-report.XXXXXX")
    XGPUI_TEST_POST_TO=$(mktemp "${TMPDIR:-/tmp}/xgpui-post-to.XXXXXX")
    export XGPUI_TEST_REPORT XGPUI_TEST_POST_TO
    self_post=1
    github_ready
fi

case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*)
        platform=windows
        start_shell_lab
        ;;
    Linux)
        platform=linux
        start_shell_lab --client-decorations
        ;;
    *) note "This test is for Fedora and Windows; nothing to do on $(uname -s)."; exit 0 ;;
esac

note "The XGPUI Shell Lab window opens. Its own title bar has window controls drawn by xgpui-shell."
note "Scroll to the 'Client-frame preview — simulation' section. Its buttons include 'Built-in controls', 'SVG on filled circle' and 'SVG, no circle'."

step "Look at the lab's own title-bar buttons without hovering them: two on the left of the title, two on the right." \
     "Each shows a visible icon (reset arrow, panel, half-filled circle, plus) in the title-bar text color, not just a grey box on hover."

if [[ $platform == linux ]]; then
    step "Look at the top of the lab window before changing anything." \
         "The lab draws its own title bar (client-side decorations) with the built-in X and minimize/maximize symbols. If the compositor drew its own title bar instead, answer skip."
fi

step "Click 'SVG on filled circle' in the preview section." \
     "Both the lab window's real title-bar controls and the preview's controls switch to small icons on grey filled circles. Button order and the set of buttons don't change."

step "Hover each real title-bar control in turn, then press and hold the mouse on Close without releasing, then drag off it and release." \
     "Each circle darkens on hover and is clearly darker again while pressed. Releasing outside Close does not close the window, and the window never starts moving."

step "Click the real Maximize control, then click it again." \
     "The window maximizes and its icon changes to the two-square Restore icon; the second click restores it and the icon changes back."

step "Click the real Minimize control, then restore the window from the taskbar or dock." \
     "The window minimizes and restores normally."

if [[ $platform == windows ]]; then
    step "Click an empty part of the lab, then press Tab repeatedly (Shift-Tab goes back)." \
         "A visible focus ring moves through the lab's buttons. It never lands on the title-bar minimize, maximize or close controls, as with native Windows title bars."
else
    step "Click an empty part of the lab, then press Tab repeatedly (Shift-Tab goes back) until the real title-bar Maximize control has a focus ring. Press Enter, then Enter again." \
         "Tab moves a visible focus ring through the lab's buttons and the title-bar controls. The first Enter maximizes the window, the second restores it."
fi

step "Switch to another app so the lab window is inactive." \
     "While the window is inactive the circles disappear and only faded icons remain; switching back brings the circles back."

step "Click 'SVG, no circle'." \
     "The real and preview controls show the same icons without circles; a circle appears only while hovering or pressing a control."

step "In the preview section, turn on 'Simulate Close hover', then click the preview's controls." \
     "The preview's Close shows its hover circle. Clicking any preview control does nothing to the real window."

step "Click 'Built-in controls'." \
     "The real and preview controls return to the built-in symbols: thin-line minimize, maximize and close, all the same size."

if [[ $platform == windows ]]; then
    step "With 'SVG on filled circle' selected, hover the real Maximize control and wait about a second." \
         "Windows 11 shows the Snap Layouts flyout under Maximize, and no 'Maximize window' tooltip covers it. On Windows 10, answer skip."
fi

step "In the preview section, click 'Buttons on left', then click it again." \
     "The first click moves the preview's controls to the left; the second turns it off and they return to the platform layout. Every choice in the lab works this way."

step "Choose a few things (for example 'Dark', 'SVG, no circle' and 'High contrast'), then click 'Reset everything' at the top right of the lab." \
     "Every choice returns to its default: the real title bar shows the built-in controls again and the preview is back to Floating, native appearance and platform buttons."

step "Click 'SVG on filled circle', then close the window with the real Close control." \
     "The window closes on the first click, and the terminal shows no 'window not found' error."

if [[ -n "${self_post:-}" ]]; then
    stop_example
    post_results "$(git branch --show-current)" "$(git rev-parse --short HEAD)" 0 \
        "$XGPUI_TEST_RESULTS" "$XGPUI_TEST_REPORT" "$XGPUI_TEST_POST_TO" 274 "${XGPUI_TEST_LOG:-}"
    rm -f "$XGPUI_TEST_REPORT" "$XGPUI_TEST_POST_TO"
fi
