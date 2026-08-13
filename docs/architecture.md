# Architecture

## Responsibility boundaries

The KnC workflow consists of independently testable stages:

1. `adams-scripts` generates KnC-specific Adams control/input files from an
   exported model.
2. Adams 2019.2 executes the event and creates a text `.res` file.
3. `adams-res-cli` converts `.res` to HDF5.
4. `chassis-lab` imports HDF5 and physical `.acd` measurements, applies
   documented conventions, and creates validation outputs.

This project does not parse `.res`, edit Adams model databases, or own the
solver invocation during the initial implementation.

## HDF5 input contract

The current supported contract is `adams-res-cli` format version 3:

```text
/
├── attrs: format_version, sample_count, signal_count, source_file
└── results/<entity>/
    ├── values             float64 [sample, component]
    ├── component_names    UTF-8 [component]
    ├── units              UTF-8 [component]
    └── adams_ids          uint64 [component]
```

Entity attributes may additionally include `original_name`, `source_path`, and
`object_id`. Signal lookup will use source path plus component name. Adams IDs
remain available for diagnostics, but are not stable semantic identifiers for
validation configuration.

The reader must reject unsupported format versions and malformed dimensions.
Changes to this contract require coordination with `adams-res-cli` and a new
format version when compatibility cannot be retained.

## Measurement input contract

Lommel `.acd` exports are tab-separated files containing:

- a physical offset row at line 2;
- normalized channel names at line 3;
- units at line 4;
- metadata followed by numeric samples.

Existing MATLAB scripts assume samples start at line 30 for VARB and line 33
for VARC. The Python parser must detect the numeric data section from content
and validate required channels rather than depending on variant-specific line
numbers.

## Canonical model

All domain evaluators consume a common signal representation:

- canonical corner names: `front_left`, `front_right`, `rear_left`,
  `rear_right`;
- numerical values with explicit units;
- source and original signal identifier;
- baseline policy: absolute, measurement offset, or static simulation;
- explicit scale and sign convention.

Conversions and sign changes belong in configuration/convention code, not in
plotting functions. This prevents repetition of the current MATLAB right-side
toe inversions and mixed radian/degree or Nm/Nmm conversions.

`chassis_lab.conventions` owns these transformations. It rejects missing
source units, applies only declared conversions, and returns transformed values
with source, original unit, target unit, corner, baseline, scale, and sign
provenance. Empty units in legacy HDF5 metadata are therefore unverified, not
implicitly converted.

## Outputs

Every run writes to a new directory under `artifacts/`:

```text
artifacts/<case>/<run-id>/
├── run.json
├── metrics.json
├── summary.csv
├── report.html
└── figures/
```

`run.json` records the input paths and checksums, database Git revision,
configuration, HDF5 format version, converter version when available, and KnC
validation version. Inputs are never overwritten.

The `create-run` command currently creates the run directory and `run.json`.
Metrics, reports, and figures are added by domain evaluators later; manifest
creation does not imply that those outputs exist yet.

SVC static evaluators use final HDF5 sample by default because Adams static
equilibrium is expected at the end of the event. The SVC longitudinal
coordinate uses front-left wheel-center X as origin; CG-X is that origin plus
rear axle mass fraction times wheelbase. Values remain unrounded internally.

## External-tool integration

The tools are configured by executable path, with command discovery through
`PATH` as the default. The eventual orchestration layer may invoke them through
`subprocess`, but analysis commands continue to accept existing HDF5 files so
that they are deterministic and usable without Adams.

The current `adams-scripts/knc` implementation is a prototype. Before it is
integrated, it needs tests and corrections for rear-wheel ID discovery, repeated
right-front control lines, duplicate variable reassignment, and a stray shell
character in the right-front lookup.
