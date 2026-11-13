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
     "Each circle darkens on hover and darkens more while pressed. Releasing outside Close does not close the window, and the window never starts moving."

step "Click the real Maximize control, then click it again." \
     "The window maximizes and its icon changes to the two-square Restore icon; the second click restores it and the icon changes back."

step "Click the real Minimize control, then restore the window from the taskbar or dock." \
     "The window minimizes and restores normally."

step "Press Tab repeatedly until a title-bar control is focused, then press Enter on Maximize." \
     "A colored focus ring is drawn around the focused control (around its circle), and Enter maximizes the window. Press Enter again to restore."

step "Switch to another app so the lab window is inactive." \
     "The control circles and icons fade while the window is inactive and return to full strength when you switch back."

step "Click 'SVG, no circle'." \
     "The real and preview controls show the same icons without circles; a circle appears only while hovering or pressing a control."

step "In the preview section, turn on 'Simulate Close hover', then click the preview's controls." \
     "The preview's Close shows its hover circle. Clicking any preview control does nothing to the real window."

step "Click 'Built-in controls'." \
     "The real and preview controls return to the built-in X and symbols, exactly as before."

if [[ $platform == windows ]]; then
    step "With 'SVG on filled circle' selected, hover the real Maximize control and wait about a second." \
         "Windows 11 shows the Snap Layouts flyout under Maximize, as it does with the built-in controls. On Windows 10, answer skip."
fi

step "Click 'SVG on filled circle', then close the window with the real Close control." \
     "The window closes on the first click."
