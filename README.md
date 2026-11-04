# ChassisLab

ChassisLab, or Chassis Laboratory, analyzes and compares vehicle chassis
simulation and measurement data. Rust owns `clab` workflow, input validation,
reports, and output safety. Embedded Python provides scientific kernels and
plotting.

## Workflow

```text
convert → inspect → analyze → compare
```

🔄 **Convert** raw Adams `.res` simulations or Lommel `.acd` measurements to
canonical HDF5. \
🔍 **Inspect** normalized files and available signals before configuring
analysis. \
📊 **Analyze** one normalized input or run manifest to calculate metrics and
curves. \
⚖️ **Compare** baseline with compatible candidates. Tolerances enable acceptance
checks.

## 👀 Preview

| Compare Plot                                                 | Debug                                                      |
| ------------------------------------------------------------ | ---------------------------------------------------------- |
| <img src="docs/assets/compare-plot-spin.png" width="450em"/> | <img src="docs/assets/debug-ride-rate.png" width="450em"/> |

## Pre-built executables

Download archive
[latest release](https://github.com/JensOlschewski/chassis-lab/releases/latest).

Extract complete archive. Keep bin/, embedded Python runtime, libraries, and
dependencies together.

```bash
bin/clab --help
```

Release archives include embedded CPython runtime and scientific dependencies.
No Python, pip, virtual environment, or Rust toolchain needed.

## Configuration ⚙️

`config.toml` defines reusable configuration file. One configuration can be
shared across simulation and measurement runs.

Configuration is modular. `config.toml` references setup, mappings, and optional
tolerance files.

| File                        | Purpose                                                             |
| --------------------------- | ------------------------------------------------------------------- |
| `config.toml`               | Main configuration file                                             |
| `setup.toml`                | General test setup.                                                 |
| `simulation-mappings.toml`  | Maps simulation source signals to ChassisLab signals and units      |
| `measurement-mappings.toml` | Maps measurement source signals to ChassisLab signals and units     |
| `tolerances.toml`           | Optional absolute and relative tolerances used by (`--tolerances`). |

`run.toml` defines input files and case metadata for each analysis or
comparison.

## Commands `>_`

### 🔄 clab convert

Convert raw simulation or measurement data to HDF5. Default output is `INPUT.h5`

```bash
clab convert input.h5
```

- `--output` for an explicit file.
- `--force` to replace an existing output.
- `--domain` (`-d`) is required and defines test domain > (`svc`|`knc`)
- `--test` (`-t`) is required for `knc` > (`vertical`|`roll`|`static`)

### 🔍 clab inspect

Inspect a measurement HDF5 file:

```bash
clab inspect examples/input/generated-measurement-knc-vertical.h5 --list
```

Create a mapping template for KnC vertical analysis:

```bash
clab inspect --mapping knc --test vertical --template
```

`--list` to show signals and \
`--match TEXT` to filter them \
`--mapping svc|knc` displays mapping requirements without opening an input file

Convert measurement example

```bash
clab convert examples/input/raw-measurement/example-measurement-knc-roll.acd \
         --domain knc --test roll \
         -o examples/input/generated-measurement-knc-roll.h5
```

### 📊 clab analyze

Analyze one normalized input or run manifest to calculate metrics and curves.

```bash
clab analyze examples/config.toml examples/input/run-simulation.toml
```

- `--report PATH` (`-r`) write a report bundle,
- `--label NAME` to explictly name the input in plot legends and report
- `--debug` for measurement branch detection diagnostics (needs `--report`).
- `--verbose` (`-v`) to print default-value warnings.
- `--force` to replace an existing report bundle.

Create report and label.

```bash
clab analyze examples/config.toml examples/input/run-simulation.toml \
    --report reports/simulation \
    --label "Simulation"
```

### ⚖️ clab compare

Compare inputs.

- `--tolerances` (`-t`) for pass/fail status from config tolerances
- `--report PATH` (`-r`) write a report bundle,
- `--label NAME` once per input, baseline first
- `--debug` for measurement branch detection diagnostics (needs `--report`).
- `--force` to replace an existing report bundle.

```bash
 clab compare examples/config.toml \
    examples/input/run-simulation.toml \
    examples/input/run-measurement.toml \
   --tolerances --report reports/compare \
   --label "Simulation" --label "Measurement"
```
