#!/usr/bin/env bash
# TEMPORARY (#119): hardware device-loss runs on Windows and Linux.
# Remove before issue-119/epic merges to main.
#
# Run from any checkout of the repository: `bash test-119.sh` (Git Bash on
# Windows). It fetches and re-runs the newest copy of itself, builds the branch
# under test in its own worktree, and leaves your checkout alone.
#
# `bash test-119.sh --post` posts the newest saved result again, without
# running anything. Every post is read back from GitHub before it counts.
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
# Always this repository, even from a clone whose origin is a fork.
REPO=gpui-luma-dev/xgpui

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

# post_comment FILE: post FILE to the issue, then read the comment back from
# GitHub and check it carries the file's `testing:` line. Offers to retry on
# any failure. Returns 0 only for a verified post.
post_comment() {
    local file="$1" marker url id attempt=1 answer
    marker="$(grep -m1 'testing:' "$file")"
    if ! command -v gh > /dev/null; then
        echo "gh is not installed. The result is saved at $file."
        return 1
    fi
    while true; do
        echo "Posting $file to $REPO#$ISSUE (attempt $attempt)..."
        if url="$(gh issue comment "$ISSUE" --repo "$REPO" --body-file "$file" 2>&1)"; then
            id="${url##*issuecomment-}"
            if [ -n "$id" ] && [ "$id" != "$url" ] \
                && gh api "repos/$REPO/issues/comments/$id" --jq .body 2>/dev/null | grep -qF "$marker"; then
                echo "Posted and verified: $url"
                return 0
            fi
            echo "gh reported '$url', but the comment could not be read back from GitHub."
        else
            echo "Posting failed:"
            echo "$url"
        fi
        if [ ! -t 0 ]; then
            echo "Not interactive, so not retrying. Post it later with: bash $SCRIPT_NAME --post"
            return 1
        fi
        read -r -p "Retry posting? [Y/n] " answer
        case "$answer" in
            n|N) echo "Not posted. Post it later with: bash $SCRIPT_NAME --post"; return 1 ;;
        esac
        attempt=$((attempt + 1))
    done
}

if [ "${1:-}" = "--post" ]; then
    LATEST_COMMENT="$(ls -t "$WORK"/comment-*.md 2>/dev/null | head -1)"
    if [ -z "$LATEST_COMMENT" ]; then
        echo "No saved result in $WORK. Run bash $SCRIPT_NAME first."
        exit 1
    fi
    echo "Newest saved result:"
    sed -n '1,12p' "$LATEST_COMMENT"
    post_comment "$LATEST_COMMENT"
    exit $?
fi

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
LOG="$WORK/results-$OS-$(uname -n)-$STAMP.txt"
# Keep the terminal on fds 3 and 4: posting results talks to it directly, so
# nothing it prints can be lost in the log pipe.
exec 3>&1 4>&2
exec > >(tee "$LOG") 2>&1

echo "testing: $TEST_BRANCH @ $COMMIT (script $SCRIPT_COMMIT)"
echo "issue #$ISSUE on $OS, $(uname -n), $(date)"
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
interval_p50() { # log -> median frame interval in ms, or nothing
    sed -n 's/.*frame interval ms p50 \([0-9.]*\).*/\1/p' "$1" | head -1
}
# expected_frames LOG SECONDS -> frames the window's own frame rate predicts.
# Thresholds use this rather than 60 fps: low-power mode (a power-saver
# profile, or a laptop on battery) caps every window at 30 fps.
expected_frames() {
    awk -v p="$(interval_p50 "$1")" -v s="$2" 'BEGIN { if (p > 0) printf "%d", s * 1000 / p; else print 0 }'
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
    elif [ "$(expected_frames "$OUT" 8)" = 0 ]; then
        record recover FAIL "no frame summary in the output"
    elif [ "$N" -lt $(( $(expected_frames "$OUT" 8) * 85 / 100 )) ]; then
        record recover FAIL "only $N of ~$(expected_frames "$OUT" 8) frames in 8 s at p50 $(interval_p50 "$OUT") ms: drawing did not resume"
    else
        record recover PASS "$N of ~$(expected_frames "$OUT" 8) frames in 8 s (p50 $(interval_p50 "$OUT") ms)"
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
            # Anything the compositor refused after the last rebuild means the
            # rebuilt engine's frames never reached the window.
            REBUILT_AT="$(grep -n 'engine rebuilt on generation' "$OUT" | tail -1 | cut -d: -f1)"
            REFUSED=""
            if [ -n "$REBUILT_AT" ]; then
                REFUSED="$(tail -n +"$REBUILT_AT" "$OUT" | grep -m1 'engine surface unsupported' | sed 's/.*engine surface unsupported: //')"
            fi
            ON_ADAPTER="$(grep 'engine rebuilt on generation' "$OUT" | tail -1 | sed -n 's/.* on \(.*\): [0-9]* dabs replayed.*/\1/p')"
            if [ -n "$PANIC" ]; then
                record tdr FAIL "panicked: $PANIC"
            elif [ "$CODE" != 0 ]; then
                record tdr FAIL "exit $CODE"
            elif [ "$ENGINE_LOST" != 0 ] && [ "$ENGINE_REBUILT" = 0 ]; then
                record tdr FAIL "the engine lost its device but never rebuilt"
            elif [ -n "$REFUSED" ]; then
                record tdr FAIL "the rebuilt engine's surface was refused ($REFUSED); rebuilt on ${ON_ADAPTER:-unknown}"
            elif [ "$LOST" != 0 ]; then
                record tdr FAIL "reported a device loss after a TDR it should survive"
            elif ! grep -q "$RECOVERED" "$OUT"; then
                record tdr FAIL "no '$RECOVERED' line: the TDR may not have reached this device"
            elif [ "$(expected_frames "$OUT" 15)" = 0 ]; then
                record tdr FAIL "no frame summary in the output"
            elif [ "$N" -lt $(( $(expected_frames "$OUT" 15) * 60 / 100 )) ]; then
                # 60%: the reset itself stalls every window for about 3 s.
                record tdr FAIL "only $N of ~$(expected_frames "$OUT" 15) frames in 15 s at p50 $(interval_p50 "$OUT") ms: drawing did not resume"
            elif [ "$ENGINE_LOST" = 0 ]; then
                record tdr PASS "$N frames in 15 s; the engine's device survived"
            else
                record tdr PASS "$N frames in 15 s; the engine rebuilt on ${ON_ADAPTER:-a new device}"
            fi
        fi
    fi
fi

# --- summary -----------------------------------------------------------------
echo "testing: $TEST_BRANCH @ $COMMIT (script $SCRIPT_COMMIT) on $OS"
for line in "${RESULTS[@]}"; do echo "  $line"; done
echo "log: $LOG"

# Stop logging and give tee a moment to flush the log file.
exec 1>&3 2>&4
sleep 1

# The comment is always written, so it can be posted by hand if needed.
COMMENT="$WORK/comment-$STAMP.md"
write_comment() { # number of log lines to include
    {
        echo "### #$ISSUE hardware run on $OS ($(uname -n))"
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
        # Long lines are cut so the comment stays under GitHub's size limit.
        tail -n "$1" "$LOG" | cut -c1-600
        echo '```'
        echo '</details>'
    } > "$COMMENT"
}
LINES=200
write_comment "$LINES"
while [ "$(wc -c < "$COMMENT")" -gt 60000 ] && [ "$LINES" -gt 20 ]; do
    LINES=$((LINES - 20))
    write_comment "$LINES"
done
echo "comment: $COMMENT ($(wc -c < "$COMMENT" | tr -d ' ') bytes, last $LINES log lines)"

echo "testing: $TEST_BRANCH @ $COMMIT (script $SCRIPT_COMMIT) on $OS"
for line in "${RESULTS[@]}"; do echo "  $line"; done
if [ -t 0 ]; then
    read -r -p "Post the summary and log tail to issue #$ISSUE? [Y/n] " ANSWER
    case "$ANSWER" in
        n|N) echo "Not posted. Post it later with: bash $SCRIPT_NAME --post" ;;
        *) post_comment "$COMMENT" ;;
    esac
else
    echo "Not interactive, so nothing was posted. Post it later with: bash $SCRIPT_NAME --post"
fi
exit "$FAILED"
