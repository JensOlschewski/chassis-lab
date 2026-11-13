# Standalone Runtime Packaging

Standalone archives use one layout on every supported platform. Platform
builders may use different native launchers and library mechanisms, but must
produce equivalent paths and metadata.

```text
clab-vVERSION-PLATFORM/
├── bin/
│   ├── clab                 # clab.exe on Windows
│   ├── mks_solver           # optional, mks_solver.exe on Windows; see below
│   └── python3.12           # platform-specific Python launcher when exposed
├── lib/python3.12/          # embedded Python and native libraries
├── licenses/                # dependency license inventory
├── manifest.json             # build and runtime provenance
└── checksums.sha256          # files inside archive
```

## Contract

Every archive must contain:

- `bin/clab` or `bin/clab.exe`.
- Embedded CPython and all required scientific wheels/native libraries. Windows
  keeps Python under `lib/python3.12/` and copies its load-time DLLs beside
  `bin/clab.exe`.
- `licenses/` with dependency license files.
- `manifest.json` with schema version, product version, target triple, source
  revision, runtime version, lock-file name and SHA-256.
- `checksums.sha256` covering every archive file except itself.

The release job also publishes an archive-level SHA-256 file beside the
archive. Archive names use `clab-vVERSION-TARGET`, where `TARGET` is the Rust
target triple: `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`, or
`x86_64-apple-darwin`.

## Solver

`clab solve` runs `mks_solver`, a MATLAB Compiler executable built per OS by
`packaging/build-mks-solver.sh` or `.ps1`. Set `MKS_SOLVER_BINARY` to that file
when running a `build-embedded-runtime` script and it is copied to `bin/` next
to `clab`, where `clab solve` and the desktop app find it. Without it the
archive has no solver. The free MATLAB Runtime R2024b must be installed on the
machine that solves; the app and `clab solve` report when it is missing.

## Locked Inputs

Each platform has its own lock file:

```text
packaging/embedded-runtime-linux-x86_64.lock
packaging/embedded-runtime-windows-x86_64.lock
packaging/embedded-runtime-macos-x86_64.lock
```

Lock files pin CPython download URL and SHA-256, plus every binary wheel and
SHA-256. Builders download only those exact inputs and verify hashes before
installation.

## Native Library Lookup

- Linux uses `$ORIGIN/../lib` in the executable RPATH.
- macOS uses `@loader_path/../lib` and must preserve compatible install names.
- Windows searches the executable directory for required DLLs.

Builders must run relocation smoke tests from a new extraction directory with
system Python, user site packages, virtual environments, and network access
unavailable. Smoke tests cover `clab convert`, SVC validation, and vertical
validation.
