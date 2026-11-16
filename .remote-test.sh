# Temporary: #107 ProPhoto RGB (media + Flowers lab). Delete before the epic merges to main.
post_to 107

note "Automated checks: color math, WGSL translation, media storage, Flowers lab"
cargo test -p xgpui-color --all-features 2>&1 | grep -E "^test result|FAILED|panicked" | report
cargo test -p xgpui-media prophoto 2>&1 | grep -E "^test result|FAILED|panicked" | report
cargo test -p xgpui-ext-lab --features native --bin color-lab flowers -- --nocapture 2>&1 \
    | grep -E "^test |Flowers pre-mapping|ProPhoto Metal|^test result" | report

note "Building color-lab (first build on a branch can take several minutes)"
cargo build -p xgpui-ext-lab --features native --bin color-lab || { echo "build failed" >&2; exit 1; }
bin="${CARGO_TARGET_DIR:-target}/debug/color-lab"
[[ -f "$bin.exe" ]] && bin="$bin.exe"
"$bin" --flowers &
XGPUI_EXAMPLE_PIDS="${XGPUI_EXAMPLE_PIDS:-} $!"
trap stop_example EXIT
sleep 5

step "Wait for the Color Lab window to show 'Flowers — WebKit ProPhoto RGB comparison' and four thumbnails" \
     "Thumbnails: sRGB reference, ProPhoto → storage, our overlay (gray photo with cyan and magenta), WebKit out-of-sRGB (orange). No error text under the photograph."
step "Read the line starting 'Before mapping:'" \
     "48.02% outside sRGB, 20.55% outside Display P3, 0.89% outside Rec.2020 (of 954000 pixels)"
step "Look at the window-mode buttons and the 'Source: … Output:' line" \
     "Windows with an HDR/wide display: Wide gamut can be selected and Output says native wide. Fedora or SDR-only: 'Current setup: standard SDR' or Standard selected, and Output says explicit sRGB preview. Neither claims full ProPhoto output."
step "Click 'Storage: P3' to switch to Rec.2020, then click 'Mapping' twice" \
     "Labels update each click; the inspector's 'Stored (…)' line changes storage name and values; the photograph keeps rendering."
step "Click 'View' three times" \
     "Cycles: sRGB | ProPhoto wipe (yellow divider) → our pre-mapping overlay → WebKit out-of-sRGB → back to the wipe."
step "Click 'Zoom' until it reads 8×" \
     "The photograph magnifies around the yellow square marker; the marker stays visible."
step "Click a red petal in the photograph" \
     "The yellow marker moves there and the inspector shows that pixel's codes, XYZ/D65 and before-mapping values with inside/OUTSIDE flags."
step "Click 'Patch blur (sRGB preview)' then 'Output' to toggle native/sRGB preview" \
     "The 13 × 2 patch strip stays visible; blur softens patch edges. Red alpha patches fade toward white."
step "Click 'Check native readback (Metal FP16)'" \
     "Not macOS: the report line explains the readback is unavailable or asks for wide/native output. It must not crash."
step "Click '← Chromaticity & metadata', then 'WebKit Flowers (ProPhoto RGB) →'" \
     "Returns to the chromaticity page, then back to Flowers with the previous selections kept."
