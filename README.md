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
.venv/bin/clab validate-svc configs/example-config.toml
```

The converter is discovered from `target/release/`, then `target/debug/`, then
standalone `crates/adams-res-converter/target/` paths, then `PATH`.

## Input

- Adams `.res` files are converted by `crates/adams-res-converter/`.
- Converted HDF5 files use format version 3.
- Lommel `.acd` files provide physical measurements.
- TOML files define cases, inputs, signal maps, setup, and tolerances.

See [`configs/example-config.toml`](configs/example-config.toml) and
[`configs/example-signals.toml`](configs/example-signals.toml). Both use
synthetic names and values as templates for private vehicle data.

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

`adams-res` remains independently usable from `crates/adams-res-converter/` or
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
configs/            commented example configuration and signal map
measures/           synthetic example reference and data policy
docs/               architecture and roadmap
```

See [`docs/architecture.md`](docs/architecture.md) and
[`docs/roadmap.md`](docs/roadmap.md).
