# ChassisLab

Vehicle simulation validation with a Rust `clab` CLI and embedded Python kernels.

## Quick start

```bash
python3 -m venv .venv
.venv/bin/python -m pip install -e './py-chassislab[dev]'
cargo build --workspace --release
```

Convert an Adams result or Lommel measurement and validate it:

```bash
target/release/clab convert input.res
target/release/clab convert measurement.acd -o measurement.h5
target/release/clab validate examples/example-config.toml
```

Default conversion output is `INPUT.simulation.h5` for `.res` and
`INPUT.measurement.h5` for `.acd`; use `--force` to replace an existing output.

## Input

- Adams `.res` files are converted by `crates/clab-converter/`.
- Converted simulation HDF5 files use format version 3.
- Converted measurement HDF5 files use schema version 1.
- Lommel `.acd` files provide physical measurements for conversion.
- TOML files define cases, inputs, signal maps, setup, and tolerances.

See [`examples/example-config.toml`](examples/example-config.toml) and
[`examples/example-signals.toml`](examples/example-signals.toml). Both use
synthetic names and values as templates for private vehicle data.
`crates/clab-converter/examples/example-vertical.acd` converts to a matching
HDF5 file; together they are a small synthetic vertical measurement pair.

Vertical validation consumes normalized inputs only:

```text
clab validate case.toml -t vertical --output-dir reports
```

SVC validation consumes simulation HDF5 v3 and reviewed reference JSON only:

```text
clab validate case.toml -d svc --output-dir reports
```

Without `-d`, `validate` runs every configured domain. `-d svc` and `-d knc`
select domains; `-t vertical` selects KnC vertical validation. `--output-dir`
creates `svc.json`, `svc.csv`, `knc-vertical.json`, `knc-vertical.csv`, and
`knc-vertical.svg` for selected domains.

SVC evaluation, tolerance comparison, reporting, provenance, and output safety
are Rust-owned. Final simulation sample is selected by default. Exit codes are
0 for pass, 1 for tolerance failure, and 2 for operational or configuration
errors. Existing Python `validate-svc` remains legacy until its Python-specific
tests and entry point are migrated.

Vertical configs now use `[vertical.inputs].measurement_hdf5`. Replace legacy
`measurement_acd` entries by converting the ACD first with `clab convert`; raw
ACD is not accepted by validation.

## Output

Commands produce:

- HDF5 result files
- JSON and CSV metrics
- deterministic SVG plots
- SHA-256 input and software provenance

Raw measurements and generated artifacts are not committed.

## Releases

Trusted `vMAJOR.MINOR.PATCH` tags publish matching Python and Rust versions as
Gitea Release assets. Releases include a Python wheel, Linux x86_64 `clab`
archive, and `SHA256SUMS`.

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
