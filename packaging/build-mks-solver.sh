#!/usr/bin/env bash
# Compiles matlab/mks_solve_main.m into a standalone mks_solver for this OS.
# Needs MATLAB with MATLAB Compiler on PATH. The output is OS-specific.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output=${1:-"$root/dist/mks-solver"}

rm -rf "$output"
mkdir -p "$output"
output=$(cd "$output" && pwd)
cd "$root/matlab"
matlab -batch "mcc -m mks_solve_main.m -a helperFunctions -o mks_solver -d '$output'"
solver="$output/mks_solver"
[ -d "$solver.app" ] && solver="$solver.app/Contents/MacOS/mks_solver"
test -x "$solver"
echo "built $solver"
