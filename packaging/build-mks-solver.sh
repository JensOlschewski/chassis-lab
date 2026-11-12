#!/usr/bin/env bash
# Compiles matlab/mks_solve_main.m into a standalone mks_solver for this OS.
# Needs MATLAB with MATLAB Compiler on PATH. The output is OS-specific.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output=${1:-"$root/dist/mks-solver"}
output=$(realpath -m "$output")

rm -rf "$output"
mkdir -p "$output"
cd "$root/matlab"
matlab -batch "mcc -m mks_solve_main.m -a helperFunctions -o mks_solver -d '$output'"
test -x "$output/mks_solver"
echo "built $output/mks_solver"
