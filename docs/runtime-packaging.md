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
`packaging/build-mks-solver.sh` or `.ps1`. The same build makes `mks_runtime`,
the small web installer behind `clab runtime install`. Set `MKS_SOLVER_DIR` and
`MKS_RUNTIME_INSTALLER_DIR` to its output folders (`dist/mks-solver`,
`dist/runtime-installer`) when running a `build-embedded-runtime` script. Both are
copied to `bin/` next to `clab`, where `clab solve`, `clab runtime install` and the
desktop app find them (on macOS as `mks_solver.app` and `mks_runtime.app`).
Without them the archive has no solver.

The MATLAB Runtime R2024b is not bundled. The app and `clab solve` offer to
install it per user on first use.

## Desktop installers and PATH

The desktop bundles carry this layout as the `clab-runtime/` resource. Each
installer exposes its `bin/clab` without moving it, because libraries resolve
relative to the real binary:

- Linux `.deb`: `postinst.sh` links `/usr/bin/clab` to
  `/usr/lib/ChassisLab/clab-runtime/bin/clab` unless a regular file is there;
  `prerm.sh` removes the link on removal.
- Windows NSIS (`installMode: currentUser`): `windows/hooks.nsh` adds
  `<install dir>\clab-runtime\bin` to the user `PATH` (HKCU, `REG_EXPAND_SZ`) and
  removes it on uninstall.
- macOS DMG: the app links `~/.local/bin/clab` at start (`cli_link.rs`). It
  repoints a link into an older ChassisLab bundle and leaves any other `clab`
  alone. Development builds have no bundled runtime and skip it.

CI checks the `.deb` only (install, `clab --version`, remove). Windows and macOS
are checked by hand: install, new terminal, `clab --version`, uninstall.

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
