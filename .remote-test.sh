# Issue #85: Adobe RGB and wide-gamut storage acceptance on Fedora and Windows.
# Run on both. Part 1 is automatic; part 2 opens the color lab for manual checks.
# Neither laptop panel is expected to offer native wide-gamut output, so both
# should fall back to standard SDR. That fallback is part of what is checked.

case "$(uname -s)" in
    Linux) os=Linux ;;
    MINGW* | MSYS* | CYGWIN*) os=Windows ;;
    *) note "This run is for Fedora and Windows; nothing to do on $(uname -s)."; exit 0 ;;
esac

# check NAME COMMAND...: run an automatic check and record PASS or FAIL in the
# summary, using the same results file as step.
check() {
    local name=$1 status
    shift
    XGPUI_STEP=$(( ${XGPUI_STEP:-0} + 1 ))
    note "[$XGPUI_STEP] $name"
    echo "\$ $*"
    "$@"
    status=$?
    if ((status == 0)); then
        printf 'PASS\t%d\t%s\t\n' "$XGPUI_STEP" "$name" >>"$XGPUI_TEST_RESULTS"
    else
        printf 'FAIL\t%d\t%s\texit status %d; see the log above\n' "$XGPUI_STEP" "$name" "$status" >>"$XGPUI_TEST_RESULTS"
    fi
}

lab=(--profile release --locked --config .cargo/validation.toml --target-dir target
     --manifest-path apps/xgpui-ext-lab/Cargo.toml --features runtime-shaders,portable-shaders,wgpu)

# start_lab NAME ARGS...: build a lab binary in release and start it in the background.
start_lab() {
    local name=$1 bin=target/release/$1
    shift
    note "Building $name (the first build on this machine can take several minutes)"
    cargo build "${lab[@]}" --bin "$name" || { echo "build of $name failed" >&2; return 1; }
    [[ -f "$bin.exe" ]] && bin="$bin.exe"
    "$bin" "$@" &
    XGPUI_EXAMPLE_PIDS="${XGPUI_EXAMPLE_PIDS:-} $!"
    trap stop_example EXIT
    echo "Started $name (pid $!)."
    sleep 3
}

note "Part 1 of 2: automatic tests on $os (no input needed; this takes a while)"

check "xgpui-color tests, ICC feature and doc tests" just test --only color
check "xgpui-gamut tests and gamut-lab tests" just test --only gamut
check "xgpui-media tests" just test --only media
if [[ $os == Linux ]]; then
    # On Windows the color-lab tests already ran as part of `just test --only color`.
    check "color-lab numerical, ICC and fixture tests" just lab-color-test
fi

note "Part 2 of 2: color lab, chromaticity workspace"
start_lab color-lab --chromaticity || exit 1

step "Find the color lab window and look at the row of buttons near the top." \
     "The heading reads Chromaticity & Color Gamut Lab. The buttons include Source: Adobe RGB (1998), Patch: Green, Storage: Rec.2020 and Map: clip."

step "Look at the line that starts with the backend name (D3D11 on Windows, Native backend on Fedora)." \
     "It says active: Default. The window opened without a crash, even though wide-gamut output is not available."

step "Look at the button row just after Nested captures + blur." \
     "Fedora: it shows ● Standard (sRGB) and the text Current setup: standard SDR. Windows: ● Standard (sRGB) is selected; a Wide gamut button may also appear (if you pick fail, note which one you see)."

step "Read the Ingestion mapping line in the Decode tracking panel on the right." \
     "It says inside storage gamut: Adobe RGB green fits in Rec.2020 storage."

step "Click Storage: Rec.2020 once so it reads Storage: P3." \
     "Ingestion mapping changes to source outside storage gamut — mapping applied. Adobe RGB green does not fit in P3."

step "Click Map: clip so it reads Map: reduce chroma." \
     "Ingestion mapping still says mapping applied, and the Stored RGBA16F numbers change. No error text appears at the bottom."

step "Click Storage and Map again so they read Storage: Rec.2020 and Map: clip, then click Patch until it reads Patch: Cyan." \
     "Ingestion mapping says inside storage gamut again."

step "Click Alpha until it reads Alpha: 0, then click it again until Alpha: 1." \
     "The numbers in Decode tracking update each time with no error. At Alpha: 0 the last Stored RGBA16F value (alpha) is 0."

step "Click Metadata: named repeatedly until it reads Metadata: ICC Adobe-compatible (Compact v4)." \
     "The Source button now reads Source: ICC colorants. Decode tracking shows numbers, not No valid pixel, and no error text appears."

step "Keep clicking Metadata through ICC sRGB (Compact v4), ICC P3 (Compact v4), ICC sRGB2014 (v2 sampled) and back to named." \
     "Each profile loads with numbers in Decode tracking and no error text."

step "Click Presentation: sRGB preview once, so it reads Presentation: sRGB preview (native wide inactive)." \
     "The chromaticity fill shows the sRGB preview and the status text says native unavailable — showing sRGB preview. Nothing crashes."

step "Click the Swatches tab at the top, then the Chromaticity tab." \
     "The settings you left (Storage, Patch, Metadata, Presentation) are unchanged."

step "Resize the window smaller and larger." \
     "The diagram and panels redraw and stay readable; nothing flickers or goes blank."

stop_example

note "Gamut lab: standard SDR fallback"
start_lab gamut-lab || exit 1

step "Look at the gamut lab window after it opens." \
     "It opens without a crash. The heading reads Gamut Lab — standard SDR baseline and the mode button says Current setup: standard SDR."

stop_example
