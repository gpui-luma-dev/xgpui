# Issue #70: Windows mouse leave and WM_SETCURSOR handling.
# Run on Windows. Fedora is unaffected by this change.

case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) ;;
    *) note "This test is for Windows only; nothing to do on $(uname -s)."; exit 0 ;;
esac

run_example anchor

note "The anchor example shows a grid of buttons. Hovering one turns it black and opens a popover beside it."

step "Hover any button, then move the pointer straight off the window onto the desktop." \
     "The button turns white again and the popover closes as soon as the pointer leaves."

step "Hover a button near the top, then move the pointer up into the window's title bar." \
     "Hover clears as you cross into the title bar, with no flicker or stuck popover; moving back down hovers buttons normally."

step "Hover a button and keep the pointer still, then press Alt+Tab to bring another window over it." \
     "When you switch back, no button is stuck black and no popover is left open."

step "Hover a button and press Win+D to show the desktop, then restore the window from the taskbar." \
     "No button is stuck black and no popover is left open after restoring."

step "Move the pointer around the buttons and the empty space between them." \
     "The cursor stays a normal arrow inside the window; it never flickers or turns into a busy cursor."

step "Move the pointer onto each window edge and corner." \
     "The resize arrows appear on edges and corners, and dragging an edge resizes the window."

step "Move the pointer from a window edge back into the window." \
     "The cursor goes back to the normal arrow straight away."
