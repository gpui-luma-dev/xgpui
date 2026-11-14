# Issue #284, package 08: Wayland activation, background effects and client
# regions from GPUI 1.28.1. Fedora (Wayland) only.
# - Activation tokens are now per request and tied to the latest real input.
# - Blur uses ext-background-effect with a KDE fallback (GNOME has neither).
# - The opaque region is only set for opaque, server-decorated windows, so
#   client-drawn shadows and rounded corners stay transparent.
# Also re-checks the #271 frame loop (presentation after startup, idle/resume).

case "$(uname -s)" in
    Linux) ;;
    *) note "This test is for Fedora (Wayland); nothing to do on $(uname -s)."; exit 0 ;;
esac

testing_line="testing: $(git rev-parse --abbrev-ref HEAD) @ $(git rev-parse --short HEAD)"
echo "$testing_line" | report
echo "session: ${XDG_SESSION_TYPE:-unknown}, desktop: ${XDG_CURRENT_DESKTOP:-unknown}" | report
if command -v wayland-info >/dev/null; then
    note "Compositor protocols relevant to this change"
    wayland-info 2>/dev/null |
        grep -oE 'xdg_activation_v1|ext_background_effect_manager_v1|org_kde_kwin_blur_manager' |
        sort -u | sed 's/^/protocol: /' | report
else
    echo "wayland-info not installed; protocol list not recorded" | report
fi

note "Wayland unit tests (build output is not posted)"
cargo test --locked --config .cargo/validation.toml -p xgpui-linux --features wayland,x11 \
    --lib linux::wayland:: 2>&1 | grep -E 'test result|FAILED|panicked' | report

start_lab() {
    local name=$1; shift
    note "Building $name (first build on a branch can take several minutes)"
    cargo build --locked --config .cargo/validation.toml --target-dir target \
        --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin "$name" \
        --features runtime-shaders,portable-shaders,wgpu ||
        { echo "build failed" >&2; return 1; }
    "target/debug/$name" "$@" &
    XGPUI_EXAMPLE_PIDS="${XGPUI_EXAMPLE_PIDS:-} $!"
    trap stop_example EXIT
    echo "Started $name (pid $!)."
    sleep 2
}

run_example animation
note "The 'animation' example opens a window with continuously moving shapes."

step "Watch the window appear, then look at the top bar or the mouse pointer for 10 seconds." \
     "The window opens focused and starts animating straight away, with no frozen first frame. No 'is ready' notification or busy/launch indicator stays on screen."

step "Minimize the window (or switch to another workspace) for 10 seconds, then bring it back." \
     "The animation resumes immediately and keeps moving smoothly; the window is not blank or frozen."

step "Drag the window's edge to resize it larger and smaller for a few seconds." \
     "The content redraws while resizing and settles at the new size with no stretched or black areas."

stop_example

start_lab shell-lab --client-decorations
note "The Shell Lab opens with its own (client-drawn) title bar, rounded corners and shadow."

step "Look at the window's corners and edges against the desktop behind it, then maximize and restore it with its own controls." \
     "Corners are rounded with the desktop showing through outside the curve, and the soft shadow is see-through, not a solid black or grey frame. Maximized, it fills the screen with square edges; restored, the rounded corners and shadow come back."

step "Resize the window from a corner, then move it around." \
     "The rounded corners and shadow follow the new size with no black rectangles or leftover old frame."

stop_example

start_lab color-lab --photo
note "Color Lab opens the photo comparison. At the bottom is a blue link: 'Italy images courtesy of WebKit’s Wide Gamut demo … ↗'."

step "Click the blue 'Italy images courtesy of WebKit…' link." \
     "Your web browser opens the WebKit page and comes to the front (or its window is highlighted as wanting attention). No busy/launch indicator remains afterwards."

step "Click the same link twice quickly, then wait 20 seconds." \
     "The browser opens the page (one or two tabs). The launch/busy indicator clears within a few seconds and does not stay until a timeout."

stop_example
