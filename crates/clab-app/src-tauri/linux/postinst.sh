#!/bin/sh
# Puts the bundled clab on PATH. A symlink keeps $ORIGIN/../lib resolving to
# the bundle. A /usr/bin/clab that is a regular file is not ours; leave it.
set -e
target=/usr/lib/ChassisLab/clab-runtime/bin/clab
if [ -x "$target" ] && { [ ! -e /usr/bin/clab ] || [ -L /usr/bin/clab ]; }; then
    ln -sf "$target" /usr/bin/clab
fi
