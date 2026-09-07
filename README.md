# ChassisLab

Python CLI for validating Adams vehicle simulation results.

## Quick start

```bash
python3 -m venv .venv
.venv/bin/python -m pip install -e './py-chassislab[dev]'
cargo build --workspace --release
```

Convert an Adams result and validate it:

```bash
.venv/bin/clab convert-res input.res -o result.h5
.venv/bin/clab validate-svc examples/example-config.toml
```

The converter is discovered from `target/release/`, then `target/debug/`, then
standalone `crates/clab-converter/target/` paths, then `PATH`.

## Input

- Adams `.res` files are converted by `crates/clab-converter/`.
- Converted simulation HDF5 files use format version 3.
- Converted measurement HDF5 files use schema version 1.
- Lommel `.acd` files provide physical measurements for conversion.
- TOML files define cases, inputs, signal maps, setup, and tolerances.

See [`examples/example-config.toml`](examples/example-config.toml) and
[`examples/example-signals.toml`](examples/example-signals.toml). Both use
synthetic names and values as templates for private vehicle data.
`examples/example-vertical.acd` and `examples/example-vertical.h5` are a small
synthetic vertical measurement pair.

## Output

Commands produce:

- HDF5 result files
- JSON and CSV metrics
- deterministic SVG plots
- SHA-256 input and software provenance

Raw measurements and generated artifacts are not committed.

## Releases

Trusted `vMAJOR.MINOR.PATCH` tags publish matching Python and Rust versions as
Gitea Release assets. Releases include a Python wheel, Linux x86_64
`adams-res` archive, and `SHA256SUMS`.

`adams-res` remains independently usable from `crates/clab-converter/` or
the release archive.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --locked
.venv/bin/ruff check py-chassislab
.venv/bin/ruff format --check py-chassislab
.venv/bin/pytest -q py-chassislab/tests
```

## Layout

```text
py-chassislab/      Python package and tests
crates/             Rust workspace crates
examples/           committed synthetic inputs and configuration
local-test-data/    ignored private or large engineering files
docs/               architecture and roadmap
```

See [`docs/architecture.md`](docs/architecture.md) and
[`docs/roadmap.md`](docs/roadmap.md).
