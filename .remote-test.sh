# Issue #284, packages 04 and 09 from GPUI 1.28.1. Windows only.
# 09: suppressing synthetic WM_MOUSEMOVE must not break real pointer input,
#     title-bar crossings or re-entry; the DirectComposition target change
#     (not topmost) must leave our content visible.
# 04: DirectWrite tabs get real advances (carets land on tab stops) and the new
#     ICU line breaking wraps mixed Latin/CJK text sensibly.

case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) ;;
    *) note "This test is for Windows; nothing to do on $(uname -s)."; exit 0 ;;
esac

testing_line="testing: $(git rev-parse --abbrev-ref HEAD) @ $(git rev-parse --short HEAD)"
echo "$testing_line" | report

note "Unit tests: synthetic-move rule and DirectWrite tabs/caret mapping (build output is not posted)"
cargo test --locked --config .cargo/validation.toml -p xgpui-windows --features test-support \
    --lib events::synthetic_mouse_move_tests 2>&1 | grep -E '^test |test result' | report
cargo test --locked --config .cargo/validation.toml -p xgpui-windows --features test-support \
    --lib direct_write:: -- --test-threads=1 2>&1 | grep -E 'tabs_preserve|test result' | report

start_lab() {
    local name=$1 features=$2
    note "Building $name (first build on a branch can take several minutes)"
    cargo build --locked --config .cargo/validation.toml --target-dir target \
        --manifest-path apps/xgpui-ext-lab/Cargo.toml --bin "$name" $features ||
        { echo "build failed" >&2; return 1; }
    local bin=target/debug/$name
    [[ -f "$bin.exe" ]] && bin="$bin.exe"
    "$bin" &
    XGPUI_EXAMPLE_PIDS="${XGPUI_EXAMPLE_PIDS:-} $!"
    trap stop_example EXIT
    echo "Started $name (pid $!)."
    sleep 2
}

run_example input
note "The 'input' example opens a window with text fields."

step "Click in the first text field, then press and hold the left mouse button over the text and drag right slowly, stop moving for two seconds while still holding, then drag a little further and release." \
     "The selection grows while you drag, stays put while you hold still, grows again when you continue, and ends where you released."

step "Move the mouse slowly back and forth over the text fields without clicking." \
     "The pointer shows a text (I-beam) cursor over the fields and a normal arrow elsewhere; nothing gets selected or moves."

step "Rest the mouse over the text without moving it, then press Shift+Left a few times with the keyboard." \
     "The keyboard extends the selection from the caret. Resting the mouse does not change it."

note "Tabs (package 04). Open Notepad, type: a, Tab, b, Tab, Tab, c  (so 'a<Tab>b<Tab><Tab>c'), select it all and copy it with Ctrl+C."

step "Click in the first text field, press Ctrl+A to select its text, then paste with Ctrl+V." \
     "The pasted text shows clear gaps where the tabs are, wider after a short word, like Notepad's tab stops. The letters do not run together."

step "Press Home, then press Right one character at a time across the whole text." \
     "Each press moves the caret one character. Across a tab the caret jumps the whole gap to the next tab stop in a single press, and it never jumps backwards or sits inside a letter."

step "Click in the middle of each tab gap, then click just left and just right of each letter." \
     "Clicking in a gap puts the caret at the nearer edge of that tab. Clicking next to a letter puts the caret beside that letter."

stop_example

run_example text_wrapper
note "The 'text_wrapper' example shows the same English, Chinese and Japanese sentence in several boxes (wrapped, ellipsis, truncate, nowrap)."

step "Look at the wrapped boxes, then drag the window's right edge slowly narrower and wider." \
     "Lines break between English words and between Chinese or Japanese characters, never inside an English word or in the middle of a character. Punctuation such as '，' stays attached to the character before it. Ellipsis boxes end in '…' and truncate boxes cut cleanly."

stop_example

start_lab shell-lab
note "The XGPUI Shell Lab opens with its own title bar and window controls."

step "Hover the lab's Close control so it highlights, then move straight up onto the window's title-bar area and back down onto the same spot of Close." \
     "Close highlights again when you come back; it is not left un-highlighted."

step "Hover Maximize, then move the pointer completely out of the window and back onto Maximize at the same spot, as exactly as you can." \
     "Maximize stops highlighting when the pointer leaves the window and highlights again when it returns."

step "Click Maximize, click it again, then drag the title bar to the top edge of the screen to Snap the window, and drag it back off." \
     "The window maximizes and restores, snaps to fill the screen and un-snaps. The lab's contents stay drawn the whole time with no black or blank areas."

step "Close the Shell Lab with its Close control." \
     "The window closes cleanly and the script continues."

start_lab gpu-interop-lab "--features native"
note "The GPU interop lab opens and draws engine-rendered content inside an XGPUI window."

step "Look at the lab window, then resize it by dragging an edge, and move it partly off-screen and back." \
     "The engine content and the XGPUI controls are visible and stacked as before: no missing content, no black window and no flicker after the resize."

step "Close the GPU interop lab." \
     "The window closes cleanly."
