# Optimize pane

Status: v1 implemented. Review open points at the end.

## Decisions

- Same pattern as Solve. The form holds `problem.toml`. Run writes a temp TOML
  for `clab optimize`. The CLI is the only validator; errors come back on stderr.
- App = UI only. No `clab-core` in the app; only `toml` + `indexmap` to
  read/write the file structure. Expressions stay text.
- `--json` = full result (like Solve's solution file).
- Runs via `worker::run_streaming` + shared `Cancel` (Stop = `cancel_solve`).
- Progress: `--progress` lines streamed verbatim to a log.

## CLI (`crates/clab-cli/src/optimize.rs`)

- `--json [PATH]` writes the full result: `design`, `start_design`,
  `objectives`, `constraints`, `J`, `start_J`, `fmincon`.
- `--json` now allowed with `--dry-run` (app shows the dry-run result).
- `--design FILE` also accepts a result file (uses its `design`).
- README updated.

## Tauri (`src-tauri/src/optimize.rs`)

- `Problem`: mirrors the TOML sections, `deny_unknown_fields`, file order kept.
- `load_problem(path)`: model paths absolute from the TOML dir.
- `save_problem(path, problem, force)`: model paths relative when inside the dir.
- `model_names(path)`: bodies + points, joints from model JSON.
- `run_optimize(request, on_line) -> {outcome, result}`:
  `optimize <problem> --progress --yes [--design f] [--dry-run] --json <out> [--force]`.
  Adjusted: the form goes to a temp TOML beside the problem file. Dry run
  writes to a scratch file.

## Frontend (`src/optimize.ts`, pane in `index.html`)

Same layout as Solve:

- Problem file (browse; loads it), Start design (optional; without it an
  `ask` dialog shows midpoints), Dry run. The result goes to
  `<problem>-optimized.json`; an existing file asks before it is replaced.
- "Adjust problem parameters": shows foldable grid tables (Solve grid: arrows,
  paste, multi-select) for Models, Optimizer, Design Variables, Signals,
  Objectives, Constraints; + Add row / × per row. Save problem… writes
  `<problem>-adjusted.toml`. Unchecked: the problem file runs as it is.
- Fuzzy completion inside expression cells and a `?` help per section.
- Optimize / Save problem… / Stop. The log shows the CLI output; no separate
  result tables.
- Icon: curve with its minimum.

## Tests

- CLI: full result written; result file as `--design`; dry run with `--json`.
- App: load/save round-trip on the example; unknown key refused; arg order;
  dry-run scratch output; `model_names` on the example model.
- `npm run build`.

## Open (review)

1. Design variables are typed rows with suggestions, not clicks in Solve-style
   tables. A plain-number cell becomes a variable in Solve: type a quoted name ("HP_L_X")
   (cell turns yellow), then Save model…, then use that model in Optimize.
2. Send result to Solve: not built; use the result file as Solve design.
3. Static help/path lists in TS can drift from `clab-core`.
4. Manual check with MATLAB pending: full run, Stop, missing runtime.
