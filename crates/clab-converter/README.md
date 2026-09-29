# clab-converter

Rust import library for Adams/Car result files (`.res`) and Lommel measurement
files (`.acd`) used by the `clab` CLI.

The library also exposes `measurement_hdf5::import_acd` for Lommel ACD files.
It writes canonical HDF5 schema version 1, retaining every channel and using the
channel named exactly `time` as `/axis/values`; imports without
that channel fail clearly.

The parser follows the structure used by the MATLAB `ResImportTool_20160629.m` reader, but it:

- scans until the first `<Step ...>` instead of using a fixed line limit;
- uses each component’s Adams `id` to map its value correctly;
- validates every numerical block against the Adams ID map;
- accepts standard and Fortran-style exponents (`E` and `D`);
- preserves original entity/component names and units as HDF5 metadata;
- refuses to overwrite output unless `--force` is passed.

## HDF5 layout

### Canonical HDF5

```text
/
├── attributes
│   ├── schema_version = 1
│   ├── source_kind = simulation | measurement
│   ├── sample_count
│   ├── signal_count
│   └── source_file
├── axis/
│   ├── attributes: kind, unit
│   └── values                   # u64 sample index or f64 time
├── signals/<safe-name>/
│   ├── values                   # f64 [sample]
│   └── attributes: original_name, unit
├── simulation/                  # simulation-only metadata
└── measurement/                 # measurement-only metadata
    └── test_info/<safe-name>/
        └── attributes: name, description, data_type, unit, value
```

Simulation signal attributes additionally retain `source_path`, `entity_name`,
`component`, `adams_id`, and optional `object_id`. Measurement signal attributes
retain `description`, `offset`, and `column`. Every signal is independently
addressable at `/signals/<safe-name>/values`; safe names use ASCII letters,
digits, `_`, and `-`, with suffixes resolving collisions.

The simulation axis is an ordinal sample index, not fabricated time. The ACD
channel named exactly `time` is the measurement axis and remains a normal signal.
Non-finite sample values are preserved.

## Local test data

Only synthetic examples belong in Git. Put private or large `.res`, `.acd`,
`.h5`, and `.hdf5` files under repository-root `local-test-data/`; it is
ignored. Tests generate their small RES fixture in temporary directories.
`examples/input/example-measurement-knc-vertical.acd` demonstrates the measurement
schema without vehicle data; convert it to generate HDF5 output.

## Build on Ubuntu or WSL

HDF5 is built from source, so a separate `libhdf5-dev` installation is not required. Install `hdf5-tools` only if you want to use the inspection commands below.

Build and test:

```bash
cargo test
cargo build --release
```

## Usage

Use the workspace `clab` command to convert raw inputs. Every `.res` file becomes
simulation HDF5; every `.acd` file becomes measurement HDF5.

Convert a file next to the source file:

```bash
target/release/clab convert BeFlex2upLoa_svc.res
```

This creates `BeFlex2upLoa_svc.h5`.

Choose another output path:

```bash
target/release/clab convert BeFlex2upLoa_svc.res --output results/BeFlex2upLoa_svc.h5
```

Replace an existing output file:

```bash
target/release/clab convert BeFlex2upLoa_svc.res --force
```

Inspect converted signals:

```bash
target/release/clab inspect BeFlex2upLoa_svc.h5 --signals
```

The tab-separated output contains the entity path, component name, unit, and
Adams ID. The library also exposes `AdamsResult::values()` for iterating over
every entity/component pair programmatically.

Inspect the result:

```bash
h5ls -r BeFlex2upLoa_svc.h5
h5dump -A BeFlex2upLoa_svc.h5
```

## Development checks

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## Scope

The parser handles the text-based Adams/Car layout represented by the supplied
MATLAB reader: an entity/component header followed by `<Step ...>` blocks.
Values use Adams component IDs when present, with header-order fallback when no
IDs exist. Mixed ID presence is rejected as ambiguous. Results are held in
memory before HDF5 output.
