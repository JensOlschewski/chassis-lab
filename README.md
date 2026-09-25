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
- Converted `.res` and `.acd` files use canonical HDF5 schema version 1.
- Canonical files explicitly declare `source_kind` as `simulation` or `measurement`.
- Lommel `.acd` files provide physical measurements for conversion.
- TOML files define combined cases, inputs, signal mappings, setup, and tolerances.

### Canonical HDF5 layout

```text
/
  attrs: schema_version, source_kind, source_file, sample_count, signal_count
/axis
  attrs: kind, unit
  values: sample indices for simulations, physical time for measurements
/signals/<safe-name>
  attrs: original_name, unit, optional source-specific metadata
  values: float64[sample_count]
/simulation
  attrs: source-format metadata
/measurement/test_info/<safe-name>
  attrs: measurement test metadata
```

Simulation axes use unitless sample indices when a RES file has no physical time;
conversion never invents time values. Signal records share one layout while retaining
simulation entity/component/ID metadata or measurement descriptions, offsets, and
test metadata as applicable.

See [`examples/example-config.toml`](examples/example-config.toml) and its
external role files. `[meta]` identifies case; `[input]`, `[setup]`,
`[mappings]`, and `[validation]` point to role files resolved relative to main
config. Input TOML assigns normalized SVC and KnC vertical paths. Mapping TOML uses
`base` plus test overrides, with shared KnC mappings inherited by vertical.
Inline function-first sections remain supported. All files use synthetic names
and values as templates for private vehicle data.
`examples/input/example-measurement-knc-vertical.acd` converts to a normalized
measurement HDF5 file; it is a small synthetic vertical measurement fixture.

Vertical validation consumes normalized inputs only:

```text
clab analyze config.toml simulation-vertical.h5 -r reports/vertical
```

SVC analysis consumes JSON or HDF5. Run manifests assign input by domain:

```text
clab analyze examples/config.toml examples/run.toml -r reports/run
```

SVC comparison evaluates baseline first, then one or more candidates. Use
`--tolerances` to activate validation tolerances already supplied by config;
without it every complete metric is informational:

```text
clab compare examples/config.toml baseline.json candidate-a.json candidate-b.json \
  --tolerances -r reports/compare
```

SVC evaluation, optional tolerance comparison, reporting, provenance, and output
safety are Rust-owned. SVC metrics without tolerances are informational.
Final simulation sample is selected by default. Embedded Python
is used only as a scientific kernel through the Rust CLI. Exit codes are 0 for
pass, 1 for tolerance failure, and 2 for operational or configuration errors.

Run manifests use `[input.svc]`, `[input.knc.vertical]`, and optional
`[input.knc.static]`, each with `file = "..."`. HDF5 `source_kind` is required;
SVC JSON must also declare it. Static is simulation HDF5
baseline for vertical normalization. Replace raw ACD with `clab convert`; raw ACD
is not accepted. Legacy domain-first configuration remains accepted during migration.

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
