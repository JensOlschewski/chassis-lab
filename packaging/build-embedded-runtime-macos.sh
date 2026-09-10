#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
lock="$root/packaging/embedded-runtime-macos-x86_64.lock"
output=${1:-"$root/dist/embed-bundle-macos-x86_64"}
output=$(realpath -m "$output")
download="$output/.wheels"
package_version=$(awk -F'"' '/^version = / { print $2; exit }' "$root/Cargo.toml")
source_revision=$(git -C "$root" rev-parse HEAD 2>/dev/null || printf 'unknown')
lock_sha256=$(shasum -a 256 "$lock" | awk '{print $1}')

rm -rf "$output"
mkdir -p "$output/bin" "$output/licenses" "$download"

source <(awk -F= '/^(CPYTHON_URL|CPYTHON_SHA256)=/{print}' "$lock")
archive="$download/cpython.tar.gz"
curl --fail --location --silent --show-error --retry 3 --retry-all-errors \
    --retry-delay 2 "$CPYTHON_URL" --output "$archive"
printf '%s  %s\n' "$CPYTHON_SHA256" "$archive" | shasum -a 256 -c --status
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
    "$python" -m pip download --only-binary=:all: --no-deps --dest "$download" "$package==$version"
    printf '%s  %s\n' "$digest" "$download/$filename" | shasum -a 256 -c --status
    "$python" -m pip install --no-index --no-deps --disable-pip-version-check \
        "$download/$filename"
done < "$lock"

RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,-rpath,@loader_path/../lib" \
    PYO3_CONFIG_FILE="$pyo3_config" \
    PYO3_PYTHON="$output/bin/python3.12" \
    cargo build --release --locked --target x86_64-apple-darwin -p clab-cli
RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,-rpath,@loader_path/../lib" \
    PYO3_CONFIG_FILE="$pyo3_config" \
    PYO3_PYTHON="$output/bin/python3.12" \
    cargo build --release --locked --target x86_64-apple-darwin \
    -p clab-converter --example embed-smoke
cp "$root/target/x86_64-apple-darwin/release/clab" "$output/bin/clab"
cp "$root/target/x86_64-apple-darwin/release/examples/embed-smoke" "$output/bin/embed-smoke"
rm "$pyo3_config"

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
(cd "$output" && find . -type f ! -name checksums.sha256 -print0 | sort -z | xargs -0 shasum -a 256) \
    > "$output/checksums.sha256"
COPYFILE_DISABLE=1 tar -czf "$output.tar.gz" -C "$(dirname "$output")" "$(basename "$output")"
shasum -a 256 "$output.tar.gz" > "$output.tar.gz.sha256"
