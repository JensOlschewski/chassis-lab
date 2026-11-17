# Compiles matlab/mks_solve_main.m into mks_solver.exe and builds the small
# MATLAB Runtime installer (mks_runtime.exe) that installs only what it needs.
# Needs MATLAB with MATLAB Compiler and the Curve Fitting Toolbox on PATH.
param([string]$Output, [string]$Installer)
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
if (-not $Output) { $Output = Join-Path $root "dist\mks-solver" }
if (-not $Installer) { $Installer = Join-Path $root "dist\runtime-installer" }
$Output = [System.IO.Path]::GetFullPath($Output)
$Installer = [System.IO.Path]::GetFullPath($Installer)

foreach ($dir in $Output, $Installer) {
    if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
    New-Item -ItemType Directory -Force $dir | Out-Null
}
Push-Location (Join-Path $root "matlab")
try {
    matlab -batch "mcc -m mks_solve_main.m -a helperFunctions -o mks_solver -d '$Output'"
    if ($LASTEXITCODE -ne 0) { throw "mcc failed" }

    if (-not (Test-Path (Join-Path $Output "mks_solver.exe"))) { throw "mks_solver.exe not built" }

    # 35160 holds the Curve Fitting functions (fnder). Without the toolbox the
    # build succeeds, but the solver fails at run time.
    if (-not (Select-String -Path (Join-Path $Output "requiredMCRProducts.txt") -Pattern 35160 -Quiet)) {
        throw "Curve Fitting Toolbox missing: install it and rebuild"
    }

    $products = Join-Path $Output "requiredMCRProducts.txt"
    matlab -batch "compiler.runtime.customInstaller('mks_runtime', '$products', OutputDir='$Installer')"
    if ($LASTEXITCODE -ne 0) { throw "customInstaller failed" }
} finally {
    Pop-Location
}
if (-not (Test-Path (Join-Path $Installer "mks_runtime.exe"))) { throw "mks_runtime.exe not built" }
Write-Host "built $Output\mks_solver.exe"
Write-Host "built $Installer\mks_runtime.exe"
