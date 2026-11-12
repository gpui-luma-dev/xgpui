#!/usr/bin/env bash
# TEMPORARY (#119): hardware device-loss runs on Windows and Linux.
# Remove before issue-119/epic merges to main.
#
# Run from any checkout of the repository: `bash test-119.sh` (Git Bash on
# Windows). It fetches and re-runs the newest copy of itself, builds the branch
# under test in its own worktree, and leaves your checkout alone.
#
# Jobs (PASS / FAIL / SKIP each):
#   unit     - the #119 unit tests for this OS.
#   recover  - an injected device loss that recovery survives: no report, and
#              the window keeps drawing.
#   give-up  - an injected loss whose recovery always fails: one report, the
#              app keeps running and exits cleanly, no panic.
#   tdr      - Windows only: a real TDR via `dxcap -forcetdr` (needs an
#              elevated shell and Graphics Tools), which recovery survives.
set -u

ISSUE=119
SCRIPT_BRANCH=issue-119/hardware-test-harness
TEST_BRANCH=issue-119/hardware-test-harness
SCRIPT_NAME=test-119.sh

# --- self-update -------------------------------------------------------------
if [ -z "${TEST_119_UPDATED:-}" ]; then
    git fetch --quiet origin "$SCRIPT_BRANCH" || { echo "git fetch failed"; exit 1; }
    LATEST="$(mktemp)"
    git show "origin/$SCRIPT_BRANCH:$SCRIPT_NAME" > "$LATEST" || { echo "cannot read $SCRIPT_NAME from origin/$SCRIPT_BRANCH"; exit 1; }
    export TEST_119_UPDATED=1
    exec bash "$LATEST" "$@"
fi

ROOT="$(git rev-parse --show-toplevel)" || exit 1
WORK="$ROOT/target/test-119"
WT="$WORK/wt"
export CARGO_TARGET_DIR="$WORK/target"
mkdir -p "$WORK"

git -C "$ROOT" fetch --quiet origin "$TEST_BRANCH" || { echo "git fetch failed"; exit 1; }
COMMIT="$(git -C "$ROOT" rev-parse --short "origin/$TEST_BRANCH")"
SCRIPT_COMMIT="$(git -C "$ROOT" rev-parse --short "origin/$SCRIPT_BRANCH")"
if [ -d "$WT" ]; then
    git -C "$WT" checkout --quiet --detach "origin/$TEST_BRANCH" || exit 1
else
    git -C "$ROOT" worktree add --quiet --detach "$WT" "origin/$TEST_BRANCH" || exit 1
fi

case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) OS=windows; EXE=.exe ;;
    Linux) OS=linux; EXE= ;;
    *) echo "This script runs on Windows (Git Bash) and Linux."; exit 1 ;;
esac

STAMP="$(date +%Y%m%d-%H%M%S)"
LOG="$WORK/results-$OS-$(hostname)-$STAMP.txt"
exec > >(tee "$LOG") 2>&1

echo "testing: $TEST_BRANCH @ $COMMIT (script $SCRIPT_COMMIT)"
echo "issue #$ISSUE on $OS, $(hostname), $(date)"
if [ "$OS" = windows ]; then
    powershell -NoProfile -Command "Get-CimInstance Win32_OperatingSystem | ForEach-Object { \$_.Caption + ' ' + \$_.Version }; Get-CimInstance Win32_VideoController | ForEach-Object { 'GPU: ' + \$_.Name + ', driver ' + \$_.DriverVersion }" 2>&1 | tr -d '\r' || true
else
    grep -m1 PRETTY_NAME /etc/os-release 2>/dev/null
    echo "session: ${XDG_SESSION_TYPE:-unknown}"
    lspci 2>/dev/null | grep -iE 'vga|3d|display' | sed 's/^/GPU: /'
    vulkaninfo --summary 2>/dev/null | grep -E 'deviceName|driverInfo' | sed 's/^ */vulkan: /'
fi
echo

RESULTS=()
FAILED=0
record() { # job, PASS|FAIL|SKIP, detail
    RESULTS+=("$(printf '%-8s %-4s %s' "$1" "$2" "$3")")
    [ "$2" = FAIL ] && FAILED=1
    echo "== $1: $2 $3"
    echo
}

cd "$WT" || exit 1
CARGO=(cargo --locked --config .cargo/validation.toml)

# --- unit --------------------------------------------------------------------
echo "== unit"
UNIT_OK=1
"${CARGO[@]}" test -p xgpui --features test-support --lib gpu_device_lost || UNIT_OK=0
if [ "$OS" = linux ]; then
    "${CARGO[@]}" test -p xgpui-linux --features wayland,x11 --lib "linux::gpu_recovery::" || UNIT_OK=0
else
    "${CARGO[@]}" test -p xgpui-windows --features test-support --lib -- directx_renderer::device_failure_tests directx_renderer::headless_retirement_tests || UNIT_OK=0
fi
[ "$UNIT_OK" = 1 ] && record unit PASS "" || record unit FAIL "see the test output above"

# --- build the workbench (debug: the fault switches are compiled out of release)
echo "== building gpu-canvas-workbench (debug)"
if ! "${CARGO[@]}" build -p xgpui-ext-lab --bin gpu-canvas-workbench; then
    record recover FAIL "workbench build failed"
    record give-up FAIL "workbench build failed"
    [ "$OS" = windows ] && record tdr FAIL "workbench build failed"
    BUILD_FAILED=1
fi
BIN="$CARGO_TARGET_DIR/debug/gpu-canvas-workbench$EXE"
export RUST_LOG="warn,gpui_windows=info,gpui_linux=info"

panic_line() { # log -> the first panic message, or nothing
    grep -m1 "panicked at" "$1" | sed 's/^thread //'
}
frames() { # log -> frames shown in the main window, or 0
    sed -n 's/.*shown: \([0-9]*\) window frames.*/\1/p' "$1" | head -1 | grep . || echo 0
}

if [ -z "${BUILD_FAILED:-}" ]; then
    if [ "$OS" = windows ]; then RECOVERED='DirectX devices successfully recreated'; else RECOVERED='GPU recovered'; fi

    # --- recover -------------------------------------------------------------
    echo "== recover: injected loss after 2 s, recovery allowed; 8 s run"
    OUT="$WORK/recover-$STAMP.log"
    XGPUI_FAULT_GPU_LOSS_AFTER=2 timeout 60 "$BIN" --demo --run-seconds 8 > "$OUT" 2>&1
    CODE=$?
    cat "$OUT"
    N="$(frames "$OUT")"
    LOST="$(grep '^gpu-device-lost: ' "$OUT" | grep -vc 'app still running' | tr -d ' ')"
    PANIC="$(panic_line "$OUT")"
    if [ -n "$PANIC" ]; then
        record recover FAIL "panicked: $PANIC"
    elif ! grep -q 'fault injection: reporting a GPU device loss' "$OUT"; then
        record recover FAIL "the injected loss never fired (exit $CODE)"
    elif [ "$CODE" != 0 ]; then
        record recover FAIL "exit $CODE"
    elif [ "$LOST" != 0 ]; then
        record recover FAIL "reported a device loss although recovery should succeed"
    elif ! grep -q "$RECOVERED" "$OUT"; then
        record recover FAIL "no '$RECOVERED' line"
    elif [ "$N" -lt 250 ]; then
        record recover FAIL "only $N frames in 8 s: drawing did not resume"
    else
        record recover PASS "$N frames in 8 s"
    fi

    # --- give-up -------------------------------------------------------------
    echo "== give-up: injected loss after 2 s, every recovery fails; 30 s limit"
    OUT="$WORK/give-up-$STAMP.log"
    START=$SECONDS
    XGPUI_FAULT_GPU_LOSS_AFTER=2 XGPUI_FAULT_GPU_RECOVERY=fail timeout 90 "$BIN" --demo --run-seconds 30 > "$OUT" 2>&1
    CODE=$?
    TOOK=$((SECONDS - START))
    cat "$OUT"
    LOST="$(grep '^gpu-device-lost: ' "$OUT" | grep -vc 'app still running' | tr -d ' ')"
    ATTEMPTS="$(grep -c 'GPU recovery attempt .* failed' "$OUT" | tr -d ' ')"
    PANIC="$(panic_line "$OUT")"
    if [ -n "$PANIC" ]; then
        record give-up FAIL "panicked: $PANIC"
    elif ! grep -q 'fault injection: reporting a GPU device loss' "$OUT"; then
        record give-up FAIL "the injected loss never fired (exit $CODE)"
    elif [ "$LOST" != 1 ]; then
        record give-up FAIL "$LOST device-lost reports (want 1)"
    elif ! grep -q 'gpu-device-lost: app still running; exiting' "$OUT"; then
        record give-up FAIL "the app did not exit from its timer after the report (exit $CODE)"
    elif [ "$CODE" != 0 ]; then
        record give-up FAIL "exit $CODE"
    elif [ "$OS" = linux ] && [ "$ATTEMPTS" != 4 ]; then
        record give-up FAIL "$ATTEMPTS retry warnings before giving up (want 4)"
    elif [ "$TOOK" -ge 30 ]; then
        record give-up FAIL "took ${TOOK}s: exited from --run-seconds, not after the report"
    else
        record give-up PASS "one report, clean exit after ${TOOK}s"
    fi

    # --- tdr (Windows) -------------------------------------------------------
    if [ "$OS" = windows ]; then
        DXCAP="$(command -v dxcap || echo /c/Windows/System32/dxcap.exe)"
        if ! net session > /dev/null 2>&1; then
            record tdr SKIP "needs an elevated (Run as administrator) Git Bash"
        elif [ ! -x "$DXCAP" ]; then
            record tdr SKIP "dxcap not found: add the 'Graphics Tools' optional feature"
        else
            echo "== tdr: real TDR after 4 s; the screen will flicker; 15 s run"
            OUT="$WORK/tdr-$STAMP.log"
            timeout 90 "$BIN" --demo --run-seconds 15 > "$OUT" 2>&1 &
            PID=$!
            sleep 4
            "$DXCAP" -forcetdr
            wait "$PID"
            CODE=$?
            cat "$OUT"
            N="$(frames "$OUT")"
            LOST="$(grep '^gpu-device-lost: ' "$OUT" | grep -vc 'app still running' | tr -d ' ')"
            PANIC="$(panic_line "$OUT")"
            ENGINE_LOST="$(grep -c 'engine device lost at' "$OUT" | tr -d ' ')"
            ENGINE_REBUILT="$(grep -c 'engine rebuilt on generation' "$OUT" | tr -d ' ')"
            if [ -n "$PANIC" ]; then
                record tdr FAIL "panicked: $PANIC"
            elif [ "$CODE" != 0 ]; then
                record tdr FAIL "exit $CODE"
            elif [ "$ENGINE_LOST" != 0 ] && [ "$ENGINE_REBUILT" = 0 ]; then
                record tdr FAIL "the engine lost its device but never rebuilt"
            elif [ "$LOST" != 0 ]; then
                record tdr FAIL "reported a device loss after a TDR it should survive"
            elif ! grep -q "$RECOVERED" "$OUT"; then
                record tdr FAIL "no '$RECOVERED' line: the TDR may not have reached this device"
            elif [ "$N" -lt 450 ]; then
                record tdr FAIL "only $N frames in 15 s: drawing did not resume"
            elif [ "$ENGINE_LOST" = 0 ]; then
                record tdr PASS "$N frames in 15 s; the engine's device survived"
            else
                record tdr PASS "$N frames in 15 s; the engine rebuilt on a new device"
            fi
        fi
    fi
fi

# --- summary -----------------------------------------------------------------
echo "testing: $TEST_BRANCH @ $COMMIT (script $SCRIPT_COMMIT) on $OS"
for line in "${RESULTS[@]}"; do echo "  $line"; done
echo "log: $LOG"

if [ -t 0 ] && command -v gh > /dev/null && gh auth status > /dev/null 2>&1; then
    read -r -p "Post the summary and log tail to issue #$ISSUE? [y/N] " ANSWER
    if [ "$ANSWER" = y ] || [ "$ANSWER" = Y ]; then
        {
            echo "### #$ISSUE hardware run on $OS ($(hostname))"
            echo
            echo "\`testing: $TEST_BRANCH @ $COMMIT (script $SCRIPT_COMMIT)\`"
            echo
            echo '```'
            for line in "${RESULTS[@]}"; do echo "$line"; done
            echo '```'
            echo
            echo '<details><summary>Log tail</summary>'
            echo
            echo '```'
            tail -n 120 "$LOG"
            echo '```'
            echo '</details>'
        } > "$WORK/comment-$STAMP.md"
        gh issue comment "$ISSUE" --body-file "$WORK/comment-$STAMP.md"
    fi
fi
exit "$FAILED"
