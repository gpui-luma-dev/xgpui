# Rust cross-checks do not invoke D3DCompile. Compile the embedded native image
# shader with the Windows SDK compiler, matching runtime SM5/strict/O3 settings.
$ErrorActionPreference = 'Stop'
$compiler = Get-ChildItem "${env:ProgramFiles(x86)}/Windows Kits/10/bin/*/x64/fxc.exe" |
    Sort-Object FullName | Select-Object -Last 1
if (-not $compiler) { throw 'Windows SDK fxc.exe is required for native HLSL validation.' }
$source = Join-Path $PSScriptRoot '../../gpui/crates/gpui-windows/src/custom_draw.hlsl'
$output = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $output | Out-Null
try {
    foreach ($stage in @(@('custom_vertex', 'vs_5_0'), @('custom_fragment', 'ps_5_0'))) {
        & $compiler.FullName /nologo /Ges /O3 /T $stage[1] /E $stage[0] /Fo "$output/$($stage[0]).cso" $source
        if ($LASTEXITCODE -ne 0) { throw "Native HLSL compilation failed: $($stage[0]) ($LASTEXITCODE)" }
    }
} finally {
    Remove-Item -Recurse -Force $output
}
