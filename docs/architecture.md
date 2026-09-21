# Architecture

## Current and target pipeline

Version `0.3.1` is the observable parity baseline during the Rust CLI rewrite.
Its current pipeline is:

```text
Adams .res / Lommel .acd
        ↓
          crates/clab-converter/ — Rust result converter
        ↓
simulation HDF5 v3 / measurement HDF5 v1
        ↓
   Rust `clab analyze` — canonical analysis and report bundles
        ↓
JSON, CSV, SVG, and run manifests
```

The target `0.4.0` pipeline is:

```text
Adams .res / HDF5 v3 / Lommel .acd / TOML
                       ↓
        Rust CLI, parsers, domain model, conventions
                       ↓
          normalized f64 arrays plus typed metadata
                       ↓
       embedded Python scientific analysis and plotting
                       ↓
              typed metrics, curves, SVG bytes
                       ↓
        Rust validation, reports, provenance, outputs
```

Rust is the application. Python is an embedded scientific capability, not a
second CLI or an owner of engineering files.

## Ownership

| Concern | Owner |
|---|---|
| CLI parsing, help, completion, errors, and exit codes | Rust |
| TOML configuration, path resolution, and reference JSON | Rust |
| Domain types, units, corners, signs, scales, and baselines | Rust |
| `.res` and `.acd` conversion; normalized HDF5 parsing | Rust |
| Conversion, orchestration, validation policy, and provenance | Rust |
| Console, JSON, CSV, SVG file handling, and overwrite safety | Rust |
| Signal processing, interpolation, fitting, and curve algorithms | Python |
| Headless deterministic plotting | Python |

Stable domain calculations that do not benefit from the scientific Python
ecosystem, including SVC calculations and tolerance comparison, belong in Rust.
Python remains appropriate for evolving NumPy/SciPy algorithms and Matplotlib
rendering.

## Rust/Python boundary

Rust calls Python through PyO3 using narrow operation-specific requests. A
request contains normalized `f64` arrays, sample axes, explicit units, setup and
algorithm parameters, and immutable source identifiers. Python returns typed
metric values, processed curves, warnings, and optional SVG bytes.

Python production modules must not receive CLI arguments or paths to TOML,
HDF5, ACD, or RES files. They must not resolve paths, convert units, select
corner/sign conventions, write reports, or create run directories. Rust
validates returned shapes and finite-value requirements before accepting output.

Initial integration may copy Rust vectors into NumPy-owned arrays. Zero-copy
sharing requires measured need and must not weaken lifetime or aliasing safety.
One interpreter is used per CLI process; subinterpreters are excluded unless the
scientific dependency stack explicitly supports them.

Python exceptions become Rust operational errors with useful operation context.
Public exit semantics remain:

- `0`: command completed successfully.
- `1`: one or more requested engineering validations failed.
- `2`: configuration, input, runtime, or output operation failed.

## CLI parity contract

The Rust CLI replaces the Python entry point without dropping `0.3.1`
capabilities:

| `0.3.1` command | Target command | Output and side effects |
|---|---|---|
| `clab inspect-h5 FILE` | `clab inspect FILE` | HDF5 summary; read-only |
| `clab list-h5-signals FILE` | `clab inspect FILE --signals` | HDF5 signal catalog; read-only |
| `clab inspect FILE` | `clab inspect FILE` | Inspect normalized inputs and mappings |
| SVC analysis | `clab analyze CONFIG RUN` | Canonical SVC metrics and report bundle |
| KnC vertical curves | `clab analyze CONFIG FILE -o DIR` | KnC vertical raw curves and SVG |

Vertical migration: `[vertical.inputs].measurement_hdf5` is required. Convert
legacy ACD inputs with `clab convert`; validation never reads raw ACD.
| `clab convert-res INPUT` | `clab convert INPUT` | Writes normalized HDF5 |

`clab inspect FILE` accepts normalized simulation or measurement HDF5 files
(`.h5` or `.hdf5`) only. Convert raw `.res` or `.acd` inputs first with
`clab convert`. `--signals` combines summary and signal listing, while `--match`
implies signal listing.

`clab analyze CONFIG RUN` analyzes configured SVC inputs. Repeatable
`--domain DOMAIN` options select a subset. This avoids conflicting boolean flags
and scales to later validation domains. Multi-domain reports use deterministic
filenames below `--output`.

Temporary hidden aliases may preserve old flat command forms during migration.
Read-only commands never convert or write implicitly. JSON and CSV fields,
console meaning, filtering, color policy, output paths, and `--force` behavior
remain compatible until an explicitly versioned contract change.

## Repository

- `crates/clab-converter/` provides Rust `.res` and `.acd` parsers and HDF5
  writers for the `clab` frontend.
- Rust workspace crates own the unified `clab` frontend and application core.
- `py-chassislab/src/chassis_lab/` currently owns validation; after migration it contains only
  embedded scientific analysis and plotting modules.
- `examples/` stores synthetic inputs, reference values, configuration, and
  signal-map TOML files; real engineering data stays local.
- `py-chassislab/tests/` contains Python and cross-language tests; Rust tests
  live beside their crates.
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
`clab-converter` imports ACD to measurement HDF5 v1 through its library API,
using channel name exactly `time` for sample axis and retaining that channel.

## Configuration

`examples/example-config.toml` defines combined SVC and vertical validation.
Canonical configuration is function-first:

```toml
[input.svc]
file = "svc.h5-or-json"

[input.knc.static]
file = "static-simulation.h5"

[input.knc.vertical]
file = "vertical.h5"

[mappings.svc]
file = "example-signals.toml"

[mappings.knc]
file = "example-vertical-signals.toml"

[mappings.knc.vertical]
# inline values add to or override domain mappings
```

Source kind comes from input metadata: HDF5 `source_kind` is optional and absent
means simulation; SVC JSON must declare it. SVC and vertical must have same source
kind. `[input.knc.static]` is run-relative simulation HDF5 baseline for vertical
normalization. Nested simulation/measurement role tables remain compatible.
External role files can replace inline sections. Input JSON combines SVC and
KnC paths; setup JSON contains setup and window data; mappings TOML contains
`[svc]` and `[knc]` bases plus test overrides; validation TOML contains optional
policy. `[meta]` contains only `name` and `loadcase`; domain is derived. Paths
resolve relative to main config.

`[setup.knc.vertical]` contains setup flags and window bounds. Signal maps contain
canonical names, entities, components, and units. Mapping precedence is shared <
domain < test; child values override, child-only values add, missing values inherit,
and nested tables deep-merge. External mapping files use same merge rules. Legacy
domain-first sections remain accepted. SVC tolerances are optional; without them,
comparisons are informational. Paths resolve relative to config file.

## Conventions

During `0.3.x`, `chassis_lab.conventions` owns units, corners, signs, scales, and
baseline policies. The Rust core assumes this ownership for `0.4.0` and passes
only normalized arrays with explicit units to Python. Missing source units remain
unverified and cannot be converted implicitly.

Rust SVC evaluation uses the final HDF5 sample by default and reads normalized
simulation files through `SimulationFile`; it does not initialize Python.
Vertical evaluation compares
measurement-offset and static-simulation X/Y/Z trajectories for all four corners
on the configured travel grid. Incomplete coverage and setup mismatches fail.

## Provenance and outputs

`create-run` writes `run.json` with resolved input paths, SHA-256 checksums, HDF5
metadata, Python and converter versions, converter source revision, and Git
revision. Reports are deterministic and never overwrite existing files without
`--force`.

Raw measurements and generated HDF5 files are local ignored inputs.

## Distribution

Standalone means one installed command with no user-managed Python, pip,
virtual environment, or Rust toolchain. The initial release artifact may contain
`clab` plus a private CPython runtime, standard library, Python modules, and
scientific native libraries. Literal one-file packaging is not required.

The runtime is located relative to the executable and initialized in isolated
mode. System `PYTHONHOME`, `PYTHONPATH`, and user site packages must not affect
execution. Release inputs are pinned and hashed; archives include checksums,
dependency licenses, and build/runtime provenance. Linux x86_64 is the first
supported bundled target. Windows and macOS follow only after native clean-host
tests pass.

## Checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --locked
.venv/bin/ruff check .
.venv/bin/ruff format --check .
.venv/bin/pytest -q
```
