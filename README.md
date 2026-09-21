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
target/release/clab analyze examples/config.toml examples/run.toml
```

Default conversion output is `INPUT.simulation.h5` for every `.res` and
`INPUT.measurement.h5` for `.acd`. Explicit output files must use `.h5` or
`.hdf5`; an existing output directory receives derived filename. Use `--force`
to replace an existing output.

## Input

- Adams `.res` files are converted by `crates/clab-converter/`.
- Converted simulation HDF5 files use format version 3.
- Converted measurement HDF5 files use schema version 1.
- Lommel `.acd` files provide physical measurements for conversion.
- TOML files define combined cases, inputs, signal mappings, setup, and tolerances.

See [`examples/example-config.toml`](examples/example-config.toml) and its
external role files. `[meta]` identifies case; `[input]`, `[setup]`,
`[mappings]`, and `[validation]` point to role files resolved relative to main
config. Input JSON combines SVC and KnC vertical paths. Mapping TOML uses
`base` plus test overrides, with shared KnC mappings inherited by vertical.
Inline function-first sections remain supported. All files use synthetic names
and values as templates for private vehicle data.
`examples/example-measurement-vertical.acd` converts to a matching
HDF5 file; together they are a small synthetic vertical measurement pair.

Vertical validation consumes normalized inputs only:

```text
clab analyze config.toml simulation-vertical.h5 -o reports/vertical
```

SVC analysis consumes readable simulation JSON or a split config/run pair:

```text
clab analyze examples/config.toml examples/run.toml -o reports/run
```

SVC comparison evaluates baseline first, then one or more candidates. Optional
TOML tolerances make configured signals pass or fail; all other signals remain
informational:

```text
clab compare examples/config.toml baseline.json candidate-a.json candidate-b.json \
  --tolerances examples/example-validation-tolerances.toml -o reports/compare
```

SVC evaluation, optional tolerance comparison, reporting, provenance, and output
safety are Rust-owned. SVC metrics without tolerances are informational.
Final simulation sample is selected by default. Embedded Python
is used only as a scientific kernel through the Rust CLI. Exit codes are 0 for
pass, 1 for tolerance failure, and 2 for operational or configuration errors.

Vertical configs use `[input.knc.vertical.measurement].input_file` for normalized
HDF5. Replace legacy `measurement_acd` entries by converting the ACD first with
`clab convert`; raw ACD is not accepted by validation. Legacy domain-first
configuration remains accepted during migration.

## Output

Commands produce:

- HDF5 result files
- JSON and CSV metrics
- deterministic SVG plots
- SHA-256 input and software provenance

Raw measurements and generated artifacts are not committed.

## Releases

Trusted `vMAJOR.MINOR.PATCH` tags publish standalone archives through GitHub
Releases for Linux x86_64, Windows x86_64, and Intel macOS x86_64. Each archive
contains `clab`, embedded CPython, scientific dependencies, licenses, build
provenance, and checksums. No user-managed Python, pip, virtual environment, or
Rust toolchain is required after extraction.

Download matching archive from [GitHub Releases](https://github.com/JensOlschewski/chassis-lab/releases),
extract it, then run `bin/clab` (`bin/clab.exe` on Windows).

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
