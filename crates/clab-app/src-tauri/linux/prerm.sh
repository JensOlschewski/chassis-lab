#!/bin/sh
set -e
target=/usr/lib/ChassisLab/clab-runtime/bin/clab
if [ "$1" = remove ] && [ "$(readlink /usr/bin/clab 2>/dev/null)" = "$target" ]; then
    rm -f /usr/bin/clab
fi
