#!/usr/bin/env bash
# Compiles matlab/mks_solve_main.m into a standalone mks_solver for this OS and
# builds the small MATLAB Runtime installer that installs only what it needs.
# Needs MATLAB with MATLAB Compiler and the Curve Fitting and Optimization Toolboxes on PATH.
# The output is OS-specific.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output=${1:-"$root/dist/mks-solver"}
installer=${2:-"$root/dist/runtime-installer"}

rm -rf "$output" "$installer"
mkdir -p "$output" "$installer"
output=$(cd "$output" && pwd)
installer=$(cd "$installer" && pwd)
cd "$root/matlab"
matlab -batch "mcc -m mks_solve_main.m -a helperFunctions -o mks_solver -d '$output'"
solver="$output/mks_solver"
[ -d "$solver.app" ] && solver="$solver.app/Contents/MacOS/mks_solver"
test -x "$solver"

# 35160 holds the Curve Fitting functions (fnder). Without the toolbox the build
# succeeds, but the solver fails at run time.
grep -q 35160 "$output/requiredMCRProducts.txt" \
  || { echo "Curve Fitting Toolbox missing: install it and rebuild" >&2; exit 1; }

matlab -batch "compiler.runtime.customInstaller('mks_runtime', '$output/requiredMCRProducts.txt', OutputDir='$installer')"
echo "built $solver"
echo "built $(ls -d "$installer"/mks_runtime*)"
