# Agent notes

## Running tests

Activate the venv first (`source .venv/bin/activate`; the pyo3 link step needs
its Python), then run the full suite with the license gate skipped:

```bash
source .venv/bin/activate
cargo test --workspace --no-fail-fast --features skip-license-gate
```

Without `--features skip-license-gate` the CLI tests write the real
`~/Library/Application Support/clab/license.json` in parallel, and fail at
random.
