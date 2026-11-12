# Compiles matlab/mks_solve_main.m into mks_solver.exe.
# Needs MATLAB with MATLAB Compiler on PATH.
param([string]$Output)
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
if (-not $Output) { $Output = Join-Path $root "dist\mks-solver" }
$Output = [System.IO.Path]::GetFullPath($Output)

if (Test-Path $Output) { Remove-Item -Recurse -Force $Output }
New-Item -ItemType Directory -Force $Output | Out-Null
Push-Location (Join-Path $root "matlab")
try {
    matlab -batch "mcc -m mks_solve_main.m -a helperFunctions -o mks_solver -d '$Output'"
    if ($LASTEXITCODE -ne 0) { throw "mcc failed" }
} finally {
    Pop-Location
}
if (-not (Test-Path (Join-Path $Output "mks_solver.exe"))) { throw "mks_solver.exe not built" }
Write-Host "built $Output\mks_solver.exe"
