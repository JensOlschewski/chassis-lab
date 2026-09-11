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

Default conversion output is `INPUT.json` for SVC `.res`, `INPUT.simulation.h5`
for vertical `.res`, and `INPUT.measurement.h5` for `.acd`; use `--force` to
replace an existing output.

## Input

- Adams `.res` files are converted by `crates/clab-converter/`.
- Converted SVC simulation JSON files use format version 1.
- Converted vertical simulation HDF5 files use format version 3.
- Converted measurement HDF5 files use schema version 1.
- Lommel `.acd` files provide physical measurements for conversion.
- TOML files define combined cases, inputs, signal mappings, setup, and tolerances.

See [`examples/example-config.toml`](examples/example-config.toml) and its
modular mapping files. Function-first sections use `[input.svc.simulation]`,
`[input.svc.measurement]`, and `[input.knc.vertical.*]`. KnC mappings use
`[mappings.knc]` as shared base and `[mappings.knc.vertical]` as test override;
precedence is shared < domain < test with recursive table merge. All files use
synthetic names and values as templates for private vehicle data.
`examples/example-measurement-vertical.acd` converts to a matching
HDF5 file; together they are a small synthetic vertical measurement pair.

Vertical validation consumes normalized inputs only:

```text
clab validate case.toml -t vertical --output-dir reports
```

SVC validation consumes readable simulation JSON and reviewed reference JSON:

```text
clab validate case.toml -d svc --output-dir reports
```

Without `-d`, `validate` runs every configured domain. `-d svc` and `-d knc`
select domains; `-t vertical` selects KnC vertical validation. `--output-dir`
creates `svc.json`, `svc.csv`, `knc-vertical.json`, `knc-vertical.csv`, and
`knc-vertical.svg` for selected domains.

Without report options, validation prints comparisons only and writes no report
files. Explicit report options enable reports.

SVC evaluation, optional tolerance comparison, reporting, provenance, and output
safety are Rust-owned. SVC references without tolerances are informational.
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
