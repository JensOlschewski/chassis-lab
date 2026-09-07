# clab-converter

Rust import library for Adams/Car result files (`.res`) and Lommel measurement
files (`.acd`). The `adams-res` binary remains a RES-only compatibility frontend.

The library also exposes `measurement_hdf5::import_acd` for Lommel ACD files.
It writes measurement schema version 1, retaining every channel and using the
channel named exactly `time` as `/measurements/sample_axis`; imports without
that channel fail clearly. The `adams-res` binary remains RES-only.

The parser follows the structure used by the MATLAB `ResImportTool_20160629.m` reader, but it:

- scans until the first `<Step ...>` instead of using a fixed line limit;
- uses each component’s Adams `id` to map its value correctly;
- validates every numerical block against the Adams ID map;
- accepts standard and Fortran-style exponents (`E` and `D`);
- preserves original entity/component names and units as HDF5 metadata;
- refuses to overwrite output unless `--force` is passed.

## HDF5 layout

### Simulation HDF5

```text
/
├── attributes
│   ├── format_version = 3
│   ├── sample_count
│   ├── signal_count
│   ├── values_per_sample
│   └── source_file
└── results/
    └── <entity>/
        ├── attributes
        │   ├── original_name
        │   ├── source_path      # when present in the Adams file
        │   └── object_id        # when present in the Adams file
        ├── values               # f64 [sample, component]
        ├── component_names      # UTF-8 [component]
        ├── units                # UTF-8 [component]
        └── adams_ids            # u64 [component]
```

Each entity has one dense matrix. Rows are Adams `<Step>` samples and columns correspond to `component_names`, `units`, and `adams_ids`. The group name uses the Adams `entity` path when available because display names are not always unique; otherwise it uses the display name. Slashes are replaced with underscores and duplicate names receive `_2`, `_3`, and so on.

Read one signal and its unit with Python:

```python
import h5py
import numpy as np

with h5py.File("results.h5", "r") as h5:
    entity = h5["results/B479_FDJ3_16z_front_whl.whl_wheel"]
    names = entity["component_names"].asstr()[:]
    column = int(np.flatnonzero(names == "VX")[0])

    signal = entity["values"][:, column]
    unit = entity["units"].asstr()[column]
```

Adams IDs remain available for validation and advanced lookup, but normal access uses the entity group and component name.

### Measurement HDF5

ACD imports write a separate measurement HDF5 v1 file. It is never combined
with simulation results.

```text
/
├── attributes: format_version, source_kind, source_file, sample_count,
│               channel_count, encoding, dialect
└── measurements/
    ├── sample_axis
    ├── channels/<safe-name>/
    │   ├── values
    │   └── attributes: original_name, description, unit, offset, column
    └── test_info/<safe-name>/
        └── attributes: name, description, data_type, unit, value
```

The ACD channel named exactly `time` becomes `sample_axis`; it remains a normal
channel too. Non-finite sample values are preserved.

## Local test data

Only synthetic examples belong in Git. Put private or large `.res`, `.acd`,
`.h5`, and `.hdf5` files under repository-root `local-test-data/`; it is
ignored. Tests generate their small RES fixture in temporary directories.
`../../examples/example-vertical.acd` and its matching HDF5 file demonstrate
the measurement schema without vehicle data.

## Build on Ubuntu or WSL

HDF5 is built from source, so a separate `libhdf5-dev` installation is not required. Install `hdf5-tools` only if you want to use the inspection commands below.

Build and test:

```bash
cargo test
cargo build --release
```

The executable is then:

```text
target/release/adams-res
```

Optionally install it into `~/.cargo/bin`:

```bash
cargo install --path .
```

## Usage

Convert a file next to the source file:

```bash
adams-res BeFlex2upLoa_svc.res
```

This creates `BeFlex2upLoa_svc.h5`.

Choose another output path:

```bash
adams-res BeFlex2upLoa_svc.res --output results/BeFlex2upLoa_svc.h5
```

Replace an existing output file:

```bash
adams-res BeFlex2upLoa_svc.res --force
```

List every available value series without creating an HDF5 file:

```bash
adams-res BeFlex2upLoa_svc.res --list
```

The tab-separated output contains the Adams ID, entity path, component name, and unit. The library also exposes `AdamsResult::values()` for iterating over every entity/component pair programmatically.

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
