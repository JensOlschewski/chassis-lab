# Example Measurements

`example-measures.json` contains synthetic SVC reference values only. It is
safe documentation data, not a measurement from a real vehicle.

The reference format is strict JSON, so it cannot contain inline comments.
Use this file as a template and keep explanations in this README or in the
commented TOML example files under `configs/`.

Real measurement references belong outside this repository. The repository
ignores raw measurement files and should not receive vehicle-specific JSON,
ACD, or HDF5 data.
