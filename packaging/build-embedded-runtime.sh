#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
lock="$root/packaging/embedded-runtime-linux-x86_64.lock"
output=${1:-"$root/dist/embed-bundle-linux-x86_64"}
download="$output/.wheels"

rm -rf "$output"
mkdir -p "$output/bin" "$output/licenses" "$download"

source <(awk -F= '/^(CPYTHON_URL|CPYTHON_SHA256)=/{print}' "$lock")
archive="$download/cpython.tar.gz"
curl --fail --location --silent --show-error "$CPYTHON_URL" --output "$archive"
printf '%s  %s\n' "$CPYTHON_SHA256" "$archive" | sha256sum --check --status
tar -xzf "$archive" --strip-components=1 -C "$output"

RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,-rpath,\$ORIGIN/../lib" \
    PYO3_PYTHON="$output/bin/python3.12" \
    cargo build --release --example embed-smoke --locked
cp "$root/target/release/examples/embed-smoke" "$output/bin/embed-smoke"

while read -r kind package filename digest; do
    [ "$kind" = WHEEL ] || continue
    version=${filename#*-}
    version=${version%%-*}
    python="$output/bin/python3.12"
    "$python" -m pip download --only-binary=:all: --no-deps \
        --dest "$download" "$package==$version"
    printf '%s  %s\n' "$digest" "$download/$filename" | sha256sum --check --status
    "$python" -m pip install --no-index --no-deps --disable-pip-version-check \
        "$download/$filename"
done < "$lock"

find "$output" -type d -name __pycache__ -prune -exec rm -rf {} +
find "$output" -name direct_url.json -delete
find "$output" -name RECORD -delete
rm -f "$output/bin/f2py" "$output/bin/fonttools" "$output/bin/numpy-config" \
    "$output/bin/pyftmerge" "$output/bin/pyftsubset" "$output/bin/ttx"

python="$output/bin/python3.12"
"$python" - <<'PY' "$output/licenses"
import pathlib
import shutil
import sys

destination = pathlib.Path(sys.argv[1])
for source in pathlib.Path(sys.prefix).rglob("*.dist-info"):
    for path in source.glob("LICENSE*"):
        shutil.copy2(path, destination / f"{source.name}-{path.name}")
    for path in source.glob("licenses/*"):
        if path.is_file():
            shutil.copy2(path, destination / f"{source.name}-{path.name}")
PY

rm -rf "$download"
tar --sort=name --mtime='UTC 1970-01-01' --owner=0 --group=0 --numeric-owner \
    -cf - -C "$(dirname "$output")" "$(basename "$output")" | gzip -n > "$output.tar.gz"
sha256sum "$output.tar.gz" > "$output.tar.gz.sha256"
