# ChassisLab

Python CLI for validating Adams vehicle simulation results.

## Quick start

```bash
python3 -m venv .venv
.venv/bin/python -m pip install -e '.[dev]'
cargo build --workspace --release
```

Convert an Adams result and validate it:

```bash
.venv/bin/clab convert-res input.res -o result.h5
.venv/bin/clab validate-svc configs/varb-lommel-curb-svc.toml
.venv/bin/clab validate-vertical configs/varb-lommel-2up.toml
```

The converter is discovered from `target/release/`, then `target/debug/`, then
legacy `reader/target/` paths, then `PATH`.

## Input

- Adams `.res` files are converted by `reader/`.
- Converted HDF5 files use format version 3.
- Lommel `.acd` files provide physical measurements.
- TOML files define cases, inputs, signal maps, setup, and tolerances.

See [`configs/example-svc.toml`](configs/example-svc.toml).

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

`adams-res` remains independently usable from `reader/` or the release archive.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --locked
.venv/bin/ruff check .
.venv/bin/ruff format --check .
.venv/bin/pytest -q
```

## Layout

```text
src/chassis_lab/   Python validation and reporting
reader/             Rust Adams result converter
configs/            case and signal-map TOML files
tests/              Python and integration tests
docs/               architecture and roadmap
```

See [`docs/architecture.md`](docs/architecture.md) and
[`docs/roadmap.md`](docs/roadmap.md).
