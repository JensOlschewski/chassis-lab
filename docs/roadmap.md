# ChassisLab Roadmap

The rewrite is incremental. Existing MATLAB output remains the historical
reference until a domain reaches reviewed parity. Version `0.3.1` is the
behavioral baseline for the Rust-owned application migration. Known MATLAB
corner mapping errors are corrected rather than copied into golden data.

## M1 Foundation

Goal: establish stable inputs, conventions, and reproducible runs.

Issues:

1. Complete the HDF5 v3 reader and semantic signal lookup API.
2. Implement a robust Lommel `.acd` parser with offsets, names, and units.
3. Define TOML case configuration and path resolution.
4. Centralize units, corner mapping, signs, and baseline policies.
5. Stabilize and test the external KnC ADM/ACF generation script.
6. Add run manifests, checksums, and isolated artifact directories.

Exit criteria:

- Synthetic tests cover valid and malformed HDF5 and ACD inputs.
- No computation depends on the working directory or a hard-coded user path.
- Every transformed signal reports its source, original unit, and convention.

## M2 SVC parity

Goal: replace `Validierung/KnC/SVC/evalSVC.m` as the first end-to-end domain.

Issues:

1. Map SVC HDF5 entities/components to canonical signal names.
2. Calculate wheel loads, axle loads, mass distribution, and total mass.
3. Calculate ride heights, tracks, wheelbase, CG, and inertias.
4. Compare VARB and VARC results with documented physical reference values.
5. Produce console, JSON, CSV, and HTML summaries.
6. Add reviewed regression fixtures and numerical tolerances.

Exit criteria:

- VARB and VARC SVC cases run from configuration.
- Unrounded calculations agree with reviewed reference values.
- Reports include provenance and explicit units.

## M3 Rust CLI and embedded Python

Goal: make Rust own the application while retaining Python for scientific signal
analysis and plotting.

Migration order:

1. Freeze the Rust/Python ownership and `0.3.1` parity contract.
2. Prove relocatable embedded CPython, NumPy, and Matplotlib packaging.
3. Add staged runner checks without disabling current CI or releases.
4. Port configuration, domain types, units, conventions, HDF5, and ACD handling
   to Rust.
5. Replace Python CLI entry points with the unified Rust `clab` command hierarchy.
6. Port stable SVC computation and reporting to Rust.
7. Complete vertical signal processing behind narrow PyO3 array API. Rust now
   owns normalized HDF5 loading, setup policy, reports, provenance, output
    safety, and `clab analyze CONFIG VERTICAL.h5 -r DIR`.
8. Port remaining SVC provenance, report serialization, and output safety to Rust. **Complete: #50**
9. Migrate remaining Python SVC CLI/report tests and remove legacy Python SVC entry points.
10. Publish standalone `v0.4.0` after complete command and numerical parity.

Exit criteria:

- Every `0.3.1` workflow has a tested Rust CLI equivalent.
- Python production modules receive normalized arrays and return scientific
  results; they parse no CLI arguments, TOML, HDF5, ACD, or RES files.
- SVC and vertical synthetic and authorized regressions remain within reviewed
  tolerances.
- Linux x86_64 runs offline after relocation without system Python, pip, or a
  virtual environment.
- Exit codes, machine-readable reports, overwrite behavior, and provenance
  remain compatible.
- Release artifacts include checksums, dependency licenses, and runtime/build
  metadata.

## M5 Vertical

Goal: establish the reusable four-corner kinematics pipeline.

Issues:

1. Implement vertical wheel-center trajectories and load curves.
2. Implement robust wheel-rate calculation with branch/noise handling.
3. Implement vertical toe and camber comparisons.
4. Generate VARB/VARC parity plots and quantitative metrics.

Exit criteria:

- Correct left/right mapping is independently tested.
- Vertical plots are generated without interactive state.
- Selected curve metrics have reviewed tolerances.

## M6 Roll

Goal: reuse the vertical pipeline for roll analysis and comparison.

Issues:

1. Reuse the vertical pipeline for roll.
2. Resolve unavailable roll `b1e1f1` results and document supported cases.

Exit criteria:

- Correct left/right mapping is independently tested.
- Roll plots are generated without interactive state.
- Selected curve metrics have reviewed tolerances.

## M7 Compliance and aligning

Goal: migrate force- and moment-driven KnC events.

Migration order:

1. Longitudinal compliance.
2. Lateral compliance.
3. Aligning moment.

Issues include loading/unloading branch detection, hysteresis-aware fitting,
mm/kN and deg/kN metrics, Nm/Nmm normalization, and documentation of missing
aligning antiphase simulation results.

## M8 Steering

Goal: validate steering geometry and effort without singular metrics.

Issues:

1. Implement steering angle/torque and front geometry comparisons.
2. Define relative toe, camber, and caster conventions.
3. Implement guarded steering-ratio calculation around zero wheel angle.
4. Validate engine-on and engine-off cases for VARB and VARC.

## M9 Automation and release

Goal: provide a maintainable private validation service.

Issues:

1. Add an optional pipeline command for generator, solver, converter, and
   validation stages.
2. Add Gitea Actions for linting and synthetic unit tests.
3. Add private-runner integration tests against authorized validation data.
4. Define synchronized Python/Rust semantic versions and HDF5 compatibility.
5. Publish tagged Python and Rust release artifacts with checksums.
6. Publish operator documentation and migration status.
7. Mark MATLAB evaluators legacy only after domain sign-off.

## M10 Desktop workspace (Tauri + web UI)

Goal: let engineers configure, run, and compare local analyses without using a
terminal, and save their workspace to resume later.

Scheduling: begin after the critical vertical/roll metrics and their numerical
regressions are reviewed (M5/M6). This milestone does not wait for all compliance
and steering work or M9 to finish; milestone numbers are identifiers, not a
strict execution sequence.

Architecture and scope:

- Use a web frontend inside Tauri with native file and folder dialogs. Data
  remains local; no hosted backend or accounts are required.
- Reuse Rust-owned configuration, validation, analysis, comparison, and output
  safety. The UI owns presentation, not engineering calculations. Keep the CLI
  supported and run long analyses in isolated worker processes.
- A saved session references inputs, configurations, and reports; it does not
  copy large engineering files or preserve running processes. Session files are
  separate from existing analysis run manifests.
- First delivery supports one complete vertical/roll workflow. Portable project
  archives, collaboration, advanced job recovery, and a general-purpose TOML
  editor are out of scope.

Small tasks, in implementation order (each suitable for a separate issue/PR):

1. **Prove the desktop runtime.** Add a minimal Tauri window and verify that an
   installed build can invoke the bundled `clab` worker with embedded Python.
   Acceptance: a synthetic analysis succeeds offline after relocation, without
   system Python; restrict frontend capabilities to the required operations.
2. **Define saved sessions.** Add a versioned session format for input/config
   references, baseline/candidates, report locations, and selected result views.
   Acceptance: round-trip tests pass; resolve relative paths from the session
   directory, reject unsupported versions, and save atomically. Keep window
   geometry and recent-session history in separate local preferences.
3. **Add session lifecycle controls.** Implement new, open, save, and save-as,
   with a dirty-state indicator.
   Acceptance: closing or replacing a session prompts for unsaved changes;
   failed saves leave the previous file intact.
4. **Select and inspect files.** Add native dialogs for configurations, inputs,
   existing reports, and output folders; expose existing inspection results.
   Acceptance: show resolved paths, source kind, signals, and actionable errors;
   missing session references can be relinked without silently substituting data.
5. **Edit the initial configuration subset.** Add forms for vertical/roll setup,
   mappings, and tolerances using existing Rust validation.
   Acceptance: opening does not rewrite TOML; explicit save/save-as preserves
   unrelated settings and modular file references, respects each file's path
   rules, and refuses to overwrite externally changed files without confirmation.
6. **Run an analysis.** Launch an isolated worker with the selected configuration,
   inputs, and output directory; show running state, diagnostics, and completion.
   Acceptance: the UI remains responsive, repeat launches cannot collide on an
   output directory, and existing overwrite protections remain enforced. Closing
   during a job requires an explicit decision; interruption is not shown as success.
7. **Browse existing and new results.** Display report metrics, units, warnings,
   provenance, and available plots without recalculating engineering values.
   Acceptance: unsupported, unavailable, failed, and tolerance-failing results
   remain distinguishable; reports can be opened without rerunning an analysis.
8. **Run comparisons and overlay curves.** Select a baseline and candidates,
   invoke existing comparison operations, and add interactive curve selection.
   Acceptance: existing compatibility checks and tolerance semantics are retained;
   overlays preserve units, corner labels, and branch identity.
9. **Resume sessions safely.** Restore saved selections and report references;
   record sufficient input/config identity to detect changes since a result ran.
   Acceptance: missing or changed dependencies are flagged, historical results are
   not presented as current, and unfinished jobs reopen as interrupted, not running.
10. **Validate and package the first desktop release.** Add a synthetic end-to-end
    test covering configure, save, analyze, compare, close, and reopen; document
    the workflow and validate packaging on each declared desktop target.
    Acceptance: keyboard-accessible controls and labeled validation errors work;
    desktop bundles include the required runtime and do not regress CLI releases.

Exit criteria:

- An engineer can complete the supported vertical/roll workflow without a terminal.
- A saved session reopens with its configuration/input references and result views,
  with clear handling of missing files, changed dependencies, and interrupted jobs.
- UI and CLI produce equivalent engineering results for the same synthetic inputs.
- No UI operation silently rewrites configuration or overwrites existing outputs.
- Desktop builds run offline without a separately installed Python environment.

## Release compatibility policy

- `pyproject.toml` and the Cargo workspace use the same `MAJOR.MINOR.PATCH`
  version while both packages remain release inputs.
- Releases use matching `vMAJOR.MINOR.PATCH` tags.
- HDF5 format versions are independent of package versions. Breaking schema
  changes require a new format version and coordinated reader/reporting support.
- Run manifests record Rust CLI version, embedded Python and scientific package
  versions, converter source revision, HDF5 format version, and repository
  revision.
- Releases publish standalone `clab` bundles for validated Linux x86_64, Windows
  x86_64, and Intel macOS x86_64 targets. No Python wheel is a user CLI
  artifact. Checksums accompany every artifact.
- `crates/clab-converter/` remains a library dependency of the unified CLI.

## Gitea maintenance policy

- The repository is private.
- `main` is protected and changes merge through pull requests.
- Milestones mirror the sections above.
- Issues contain acceptance criteria and links to relevant legacy scripts.
- Large model, measurement, `.res`, and generated `.h5` files are not stored in
  this repository.
- Normal CI uses synthetic fixtures; authorized real data stays on a private
  runner.
- Existing runner checks remain active during migration. Bundled-runtime and
  parity jobs are added before superseded Python CLI jobs are removed.
- Stable states are tagged as `v0.x.y` during migration and `v1.0.0` after all
  supported domains reach sign-off.
- `v*` tags are protected; only trusted maintainers can trigger release jobs.
