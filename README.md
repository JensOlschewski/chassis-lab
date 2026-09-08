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
.venv/bin/clab validate-svc examples/example-config.toml
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
clab validate vertical case.toml --plot-output trajectories.svg
```

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
