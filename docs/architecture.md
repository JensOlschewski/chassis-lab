# Architecture

## Pipeline

```text
Adams .res / Lommel .acd
        ↓
converter/ — Rust result converter
        ↓
HDF5 format 3
        ↓
src/chassis_lab/ — Python validation
        ↓
JSON, CSV, SVG, and run manifests
```

## Repository

- `converter/` parses Adams `.res` files and writes normalized HDF5.
- `src/chassis_lab/` reads HDF5 and Lommel `.acd` measurements.
- `configs/` stores case and reusable signal-map TOML files.
- `tests/` contains Python, Rust, and cross-language tests.
- `artifacts/` stores generated results and remains untracked.

## HDF5 contract

Supported format version: `3`.

```text
/
├── attrs: format_version, sample_count, signal_count, source_file
└── results/<entity>/
    ├── values             float64 [sample, component]
    ├── component_names    UTF-8 [component]
    ├── units              UTF-8 [component]
    └── adams_ids          uint64 [component]
```

Signal lookup uses source path plus component. Adams IDs remain diagnostic
metadata, not stable configuration identifiers. `AdamsHdf5` rejects unsupported
versions and malformed dimensions.

## Measurements

Lommel `.acd` files contain metadata, offsets, channel names, units, and numeric
samples. The reader detects data boundaries instead of relying on VARB/VARC line
numbers. It preserves `NaN` and `Inf`; evaluators decide whether they are valid.

## Configuration

Case files define vehicle metadata once, then domain inputs:

```toml
[svc.inputs]
signal_map = "signals/svc.toml"
simulation_hdf5 = "..."
measurement_reference = "...json"

[vertical.inputs]
signal_map = "signals/knc.toml"
measurement_acd = "...acd"
simulation_hdf5 = "...h5"
```

`[vertical.setup]` contains setup flags and `window_*_mm` bounds. Signal maps
contain canonical names, HDF5 entities, components, and units. Inline mappings
remain available for isolated fixtures, but cannot be combined with `signal_map`.

## Conventions

`chassis_lab.conventions` owns units, corners, signs, scales, and baseline
policies. `apply_convention` returns transformed values with provenance. Missing
source units remain unverified and cannot be converted implicitly.

SVC evaluators use the final HDF5 sample by default. Vertical evaluation compares
measurement-offset and static-simulation X/Y/Z trajectories for all four corners
on the configured travel grid. Incomplete coverage and setup mismatches fail.

## Provenance and outputs

`create-run` writes `run.json` with resolved input paths, SHA-256 checksums, HDF5
metadata, Python and converter versions, converter source revision, and Git
revision. Reports are deterministic and never overwrite existing files without
`--force`.

Raw measurements and generated HDF5 files are local ignored inputs.

## Checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --locked
.venv/bin/ruff check .
.venv/bin/ruff format --check .
.venv/bin/pytest -q
```
