#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
lock="$root/packaging/embedded-runtime-linux-x86_64.lock"
output=${1:-"$root/dist/embed-bundle-linux-x86_64"}
output=$(realpath -m "$output")
download="$output/.wheels"
package_version=$(awk -F'"' '/^version = / { print $2; exit }' "$root/Cargo.toml")
source_revision=$(git -C "$root" rev-parse HEAD 2>/dev/null || printf 'unknown')
lock_sha256=$(sha256sum "$lock" | awk '{print $1}')

rm -rf "$output"
mkdir -p "$output/bin" "$output/licenses" "$download"

source <(awk -F= '/^(CPYTHON_URL|CPYTHON_SHA256)=/{print}' "$lock")
archive="$download/cpython.tar.gz"
curl --fail --location --silent --show-error --retry 3 --retry-all-errors \
    --retry-delay 2 "$CPYTHON_URL" --output "$archive"
printf '%s  %s\n' "$CPYTHON_SHA256" "$archive" | sha256sum --check --status
tar -xzf "$archive" --strip-components=1 -C "$output"

pyo3_config="$output/pyo3-config"
cat > "$pyo3_config" <<EOF
implementation=CPython
version=3.12
shared=true
abi3=false
lib_name=python3.12
lib_dir=$output/lib
executable=$output/bin/python3.12
pointer_width=64
EOF
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

RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,-rpath,\$ORIGIN/../lib" \
    PYO3_CONFIG_FILE="$pyo3_config" \
    PYO3_PYTHON="$output/bin/python3.12" \
    cargo build --release --locked -p clab-cli
cp "$root/target/release/clab" "$output/bin/clab-bin"
cat > "$output/bin/clab" <<'SH'
#!/bin/sh
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
unset PYTHONHOME PYTHONPATH VIRTUAL_ENV
export PYTHONHOME="$root"
exec "$root/bin/clab-bin" "$@"
SH
chmod +x "$output/bin/clab"
rm "$pyo3_config"

find "$output" -type d -name __pycache__ -prune -exec rm -rf {} +
find "$output" -name direct_url.json -delete
find "$output" -name RECORD -delete
rm -f "$output/bin/f2py" "$output/bin/fonttools" "$output/bin/numpy-config" \
    "$output/bin/pyftmerge" "$output/bin/pyftsubset" "$output/bin/ttx"

python="$output/bin/python3.12"
"$python" - "$output/manifest.json" "$package_version" "$source_revision" \
    "$lock_sha256" "$lock" <<'PY'
import json
import pathlib
import sys

manifest_path = pathlib.Path(sys.argv[1])
manifest = {
    "schema_version": 1,
    "product": "clab",
    "product_version": sys.argv[2],
    "target": "x86_64-unknown-linux-gnu",
    "platform": "linux-x86_64",
    "build_profile": "release",
    "source_revision": sys.argv[3],
    "runtime": {
        "implementation": "CPython",
        "version": "3.12",
        "lock_file": pathlib.Path(sys.argv[5]).name,
        "lock_sha256": sys.argv[4],
    },
    "layout": {
        "executable": "bin/clab",
        "embedded_python": "bin/python3.12",
        "licenses": "licenses/",
        "checksums": "checksums.sha256",
    },
}
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
PY
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
(cd "$output" && find . -type f ! -name checksums.sha256 -print0 | sort -z | xargs -0 sha256sum) \
    > "$output/checksums.sha256"
tar --sort=name --mtime='UTC 1970-01-01' --owner=0 --group=0 --numeric-owner \
    -cf - -C "$(dirname "$output")" "$(basename "$output")" | gzip -n > "$output.tar.gz"
sha256sum "$output.tar.gz" > "$output.tar.gz.sha256"
