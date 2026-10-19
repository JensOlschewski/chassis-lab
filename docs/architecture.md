# Architecture

## Current and target pipeline

Version `0.3.1` is the observable parity baseline during the Rust CLI rewrite.
Its current pipeline is:

```text
Adams .res / Lommel .acd
        ↓
          crates/clab-converter/ — Rust result converter
        ↓
canonical HDF5 v1 (`simulation` or `measurement`)
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
| KnC vertical curves | `clab analyze CONFIG FILE -r DIR` | KnC vertical raw curves and SVG |

Vertical migration: `[vertical.inputs].measurement_hdf5` is required. Convert
legacy ACD inputs with `clab convert`; validation never reads raw ACD.
| `clab convert-res INPUT` | `clab convert INPUT -d DOMAIN [-t TEST]` | Writes normalized HDF5 with `domain` and `test` |

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
- `py-chassislab/src/chassis_lab/` contains the embedded KnC kernels:
  `knc_kernel.py` (branch detection, sweep gradients, interpolation, secant
  metric catalog evaluation), `knc_plot_kernel.py` (report plotting), and one
  thin test kernel each, `knc_vertical_kernel.py` and `knc_roll_kernel.py`,
  holding that test's metric and plot catalog. Rust loads the core, the plot
  kernel when the test plots, and the test kernel.
- `examples/` stores synthetic inputs, reference values, configuration, and
  signal-map TOML files; real engineering data stays local.
- `py-chassislab/tests/` contains Python and cross-language tests; Rust tests
  live beside their crates.
- `artifacts/` stores generated results and remains untracked.

## HDF5 contract

Supported canonical schema version: `1`.

```text
/
├── attrs: schema_version (2), source_kind, domain, test, sample_count, signal_count, source_file
├── axis/attrs: kind, unit
├── axis/values            uint64 [sample_index] or float64 [time]
├── signals/<safe-name>/values
└── measurement/test_info/<safe-name>/
```

Signal lookup uses source path plus component for simulations and original or
safe name for measurements. Adams IDs remain diagnostic metadata, not stable
configuration identifiers. Canonical reader rejects unsupported versions and
malformed dimensions.

## Measurements

Lommel `.acd` files contain metadata, offsets, channel names, units, and numeric
samples. The reader detects data boundaries instead of relying on VARB/VARC line
numbers. It preserves `NaN` and `Inf`; evaluators decide whether they are valid.
`clab-converter` imports ACD to canonical HDF5 v1 through its library API,
using channel name exactly `time` for sample axis and retaining that channel.

## Configuration

`examples/config.toml` defines combined SVC and vertical validation.
Canonical configuration is function-first:

```toml
[input.svc]
file = "svc.h5-or-json"

[input.knc.static]
file = "static-simulation.h5"

[input.knc.vertical]
file = "vertical.h5"

[input.knc.roll]
file = "roll.h5"

[mappings.simulation]
file = "simulation-mappings.toml"

[mappings.measurement]
file = "measurement-mappings.toml"
```

Source kind comes from required input metadata: HDF5 and SVC JSON `source_kind`
must declare it. SVC and KnC tests must have same source
kind. `[input.knc.static]` is run-relative simulation HDF5 baseline for vertical
and roll normalization. Roll axle signals (`total_roll_angle`,
`front|rear_suspension_roll_angle`, `front|rear|total_roll_moment`,
`front|rear|total_suspension_roll_moment`) keep absolute values; only corner
signals are normalized. Every normalized curve records the subtracted
`offset`, so absolute positions stay recoverable. Roll does not yet combine
with SVC or vertical in one run manifest; `compare` accepts roll inputs on
their own. Nested simulation/measurement role tables remain compatible.
External role files can replace inline sections, at any depth — a whole
`source_kind` under `[mappings]`, one domain under it, or one test under that
domain, can each point at its own file (`examples/` combines every domain
into one file per source_kind, `simulation-mappings.toml` /
`measurement-mappings.toml`; splitting further per domain or test, or
combining into one file overall, works identically — see `merged_mapping` in
`crates/clab-core/src/config.rs`). Input JSON combines SVC and
KnC paths; setup JSON contains setup and window data; mappings TOML contains
`[simulation]` and `[measurement]` bases plus domain and test overrides;
validation TOML contains optional policy. `[meta]` contains optional run `name` and `loadcase`; domain is derived.
Run metadata overrides shared configuration metadata, and absent values report
as `undefined`. Paths resolve relative to main config.

KnC setup lives under `[setup.knc]`; `[setup.knc.vertical]` and
`[setup.knc.roll]` hold test-only values and override `[setup.knc]` key by key
(nested tables deep-merge).
`[meta]` holds the test setup flags (`brakes_applied`, `engine_running`,
`front_arb_connected`, optional `rear_arb_connected`), which also appear in the
report footer. `[window]` defines the plot range (`minimum_mm`, `maximum_mm`,
`step_mm`; legacy `travel_minimum_mm` and `travel_maximum_mm` remain accepted) and
`plot_branches` (`all` or `selected`). `[metric]` defines the metric computation:
`gradient_points_mm` (default 10, limited to the travel window and required to lie
inside it when set), `gradient_points_deg` (roll only, default 1), `direction` (`rise`, `fall`, `any`) and `index` (default 2).
`[metric.measurement]` and `[metric.simulation]` override `direction` and `index`
per source kind; `[window.measurement]` and `[window.simulation]` do the same for
`plot_branches`. Setting flags directly under `[setup.knc]`, the `[branch]` table
and `metric_gradient_points_mm` in the window remain accepted. Optional keys that
are left out use their default and produce one analysis warning each.
`sprung_mass_front_left_kg`/`..._front_right_kg`/`..._rear_left_kg`/`..._rear_right_kg`
in `[metric]` are the `ride_frequency` metric's per-corner mass fallback: this run's
own SVC `<corner>_mass_kg` is used automatically when SVC and KnC vertical run
together, and a configured value overrides it. Signal maps contain
canonical names, entities, components, and units. Every mapping table ends
in the same shape: `source_kind.domain[.test][.signals].<canonical_name>`
(`source_kind` is `simulation` or `measurement`, always the outermost key),
so a simulation and a measurement table for the same domain/test sit side by
side instead of one being an unmarked default — e.g. `[simulation.svc]` /
`[measurement.svc]`, `[simulation.knc.vertical]` /
`[measurement.knc.vertical]`. The `.signals` level and the trailing
canonical-name level can both collapse — `[simulation.knc.vertical.signals.x]`,
`[simulation.knc.vertical.x]`, and a single flat
`[simulation.knc.vertical.x]` header all resolve the same way; `unit`/`entity`
(and `component` for simulation) always sit one level below the canonical
name. Mapping precedence is shared < domain < test; child values override,
child-only values add, missing values inherit, and nested tables deep-merge.
External mapping files use the same merge rules. SVC tolerances are
optional; without them, comparisons are informational. Paths resolve
relative to config file.

Measurement sources resolve through mapping tables the same way simulation
sources do, just without a `component`: a flat channel or JSON key has
nothing to split off. `[measurement.knc.vertical]` maps canonical
KnC vertical names to HDF5 measurement channel names (`entity` + `unit`);
`[measurement.svc]` maps canonical SVC metric names
(`clab_core::conventions::SVC_METRICS`) to SVC measurement reference JSON
keys (`entity`; `unit` is informational only there, since reference values
are already final). There is no hardcoded fallback: a required canonical
name absent from the mapping is a config error (KnC vertical wheel-center
signals, every SVC simulation signal); a JSON key absent from the mapping
is a reference-file error (SVC measurement). KnC vertical's other
measurement signals (`wheel_load`, `toe`, `camber`, `spin`, `wheel_pad_z`,
`tire_deflection`, `contact_patch_x/y`) are optional the same way their
simulation counterparts are: missing from the mapping, or mapped but absent
from the file, just skips that curve.

## Conventions

During `0.3.x`, `chassis_lab.conventions` owns units, corners, signs, scales, and
baseline policies. The Rust core assumes this ownership for `0.4.0` and passes
only normalized arrays with explicit units to Python. Missing source units remain
unverified and cannot be converted implicitly.

Rust SVC evaluation uses the final HDF5 sample by default and reads normalized
simulation files through `CanonicalFile`; it does not initialize Python.
Vertical evaluation compares
measurement-offset and static-simulation X/Y/Z trajectories for all four corners
on the configured travel grid. Incomplete coverage and setup mismatches fail.

KnC metrics are secants between symmetric points on the configured branch
sweep: vertical selects the sweep on each corner's wheel-center travel, roll on
`total_roll_angle`. Roll wheel-travel metrics use `gradient_points_mm`,
roll-angle metrics `gradient_points_deg` (default 1 deg). Roll metrics:
`roll_steer|roll_camber_<corner>_deg_per_mm` (over wheel-center Z) and
`_deg_per_deg` (over the axle's `suspension_roll_angle`),
`roll_wheel_rate_<corner>_n_per_mm`, `roll_wheel_rate_without_arb_<corner>_n_per_mm`
(only for an axle whose `<axle>_arb_connected` is false),
`roll_stiffness_<axle>_nm_per_deg` (front, rear, total; roll moment over
`total_roll_angle`), `roll_stiffness_distribution_front_pct` (total falls back
to front + rear), and per axle `roll_center_height_<axle>_mm`,
`roll_center_height_change_<axle>_mm_per_deg` and
`roll_center_lateral_migration_<axle>_mm_per_deg`. The roll center follows the
legacy dissertation method: contact-path angles from d(lateral contact
position)/d(wheel pad Z) per side, corrected by roll angle, close a triangle
over the absolute contact points (law of sines). The lateral position is
`center_of_pressure_y`, else `contact_patch_y` with a warning. The configured
sweep and its adjacent opposite sweep are each smoothed by a cubic
least-squares fit over roll angle and averaged into the hysteresis mid line.

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
