//! Shared plumbing for the commands that run the compiled MATLAB solver:
//! finding the solver and the MATLAB Runtime, and writing JSON output.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;

pub(crate) const RUNTIME_RELEASE: &str = "R2024b";
const SOLVER_ENV: &str = "CLAB_MKS_SOLVER";
const RUNTIME_ENV: &str = "CLAB_MATLAB_RUNTIME";
const SOLVER_NAME: &str = if cfg!(windows) {
    "mks_solver.exe"
} else {
    "mks_solver"
};

/// `--json` without a value writes to the current directory; a directory
/// (existing, or written with a trailing separator) receives the default name.
pub(crate) fn output_path(requested: Option<&Path>, input: &Path, kind: &str) -> Result<PathBuf> {
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    let name = format!("{stem}-{kind}.json");
    let path = match requested {
        None => PathBuf::from(name),
        Some(path) if path.is_dir() || path.to_string_lossy().ends_with(['/', '\\']) => {
            path.join(name)
        }
        Some(path) => path.to_path_buf(),
    };
    absolute(&path)
}

/// A file in the output directory, so it can replace the output atomically on success.
pub(crate) fn scratch_file(output: Option<&Path>) -> Result<tempfile::NamedTempFile> {
    let mut scratch = tempfile::Builder::new();
    #[cfg(unix)]
    scratch.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o644));
    match output.and_then(Path::parent) {
        Some(dir) => scratch.tempfile_in(dir),
        None => scratch.tempfile(),
    }
    .context("cannot create a scratch file")
}

/// Indents objects and arrays of containers; arrays of scalars stay on one line.
pub(crate) fn format_json(value: &Value, depth: usize, out: &mut String) {
    match value {
        Value::Array(items) if items.iter().any(|v| v.is_array() || v.is_object()) => {
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                out.push_str(&"  ".repeat(depth + 1));
                format_json(item, depth + 1, out);
                out.push_str(if index + 1 < items.len() { ",\n" } else { "\n" });
            }
            out.push_str(&"  ".repeat(depth));
            out.push(']');
        }
        Value::Object(map) if !map.is_empty() => {
            out.push_str("{\n");
            for (index, (key, item)) in map.iter().enumerate() {
                out.push_str(&"  ".repeat(depth + 1));
                out.push_str(&Value::from(key.as_str()).to_string());
                out.push_str(": ");
                format_json(item, depth + 1, out);
                out.push_str(if index + 1 < map.len() { ",\n" } else { "\n" });
            }
            out.push_str(&"  ".repeat(depth));
            out.push('}');
        }
        scalar_or_flat => out.push_str(&scalar_or_flat.to_string()),
    }
}

pub(crate) fn absolute(path: &Path) -> Result<PathBuf> {
    std::path::absolute(path).with_context(|| format!("invalid path: {}", path.display()))
}

pub(crate) fn file_path(path: &Path, what: &str) -> Result<PathBuf> {
    if !path.is_file() {
        bail!("{what} file does not exist: {}", path.display());
    }
    absolute(path)
}

pub(crate) fn find_solver() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(SOLVER_ENV).map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
        bail!("{SOLVER_ENV} does not point to a file: {}", path.display());
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| solver_in(exe.parent()?))
        .with_context(|| {
            format!("solver not found: install {SOLVER_NAME} next to clab or set {SOLVER_ENV}")
        })
}

/// MATLAB Compiler builds an app bundle on macOS, so look inside it too.
pub(crate) fn solver_in(dir: &Path) -> Option<PathBuf> {
    [
        dir.join(SOLVER_NAME),
        dir.join("mks_solver.app/Contents/MacOS/mks_solver"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

pub(crate) fn find_runtime() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(RUNTIME_ENV).map(PathBuf::from) {
        if path.is_dir() {
            return Ok(path);
        }
        bail!(
            "{RUNTIME_ENV} does not point to a directory: {}",
            path.display()
        );
    }
    let candidates: &[&str] = if cfg!(windows) {
        &["C:\\Program Files\\MATLAB\\MATLAB Runtime"]
    } else if cfg!(target_os = "macos") {
        &["/Applications/MATLAB/MATLAB_Runtime"]
    } else {
        &["/usr/local/MATLAB/MATLAB_Runtime", "/usr/local/MATLAB"]
    };
    candidates
        .iter()
        .map(|dir| Path::new(dir).join(RUNTIME_RELEASE))
        .chain(user_runtime_dir().map(|dir| dir.join(RUNTIME_RELEASE)))
        .find(|path| path.is_dir())
        .with_context(|| {
            format!(
                "MATLAB Runtime {RUNTIME_RELEASE} not found: run `clab runtime install` or set {RUNTIME_ENV}"
            )
        })
}

/// Where `clab runtime install` puts the Runtime. It needs no administrator rights.
pub(crate) fn user_runtime_dir() -> Option<PathBuf> {
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        // Not "Application Support": the macOS installer silently installs nothing into a path with a space.
        env("HOME").map(|home| home.join("Library"))
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|home| home.join(".local/share")))
    };
    Some(base?.join("ChassisLab/matlab-runtime"))
}

/// Library search path variable the solver needs to find the Runtime.
/// Windows needs none: the Runtime installer adds itself to `PATH`.
pub(crate) fn library_path(runtime: &Path) -> Result<Option<(&'static str, OsString)>> {
    let (var, dirs): (&str, &[&str]) = if cfg!(windows) {
        return Ok(None);
    } else if cfg!(target_os = "macos") {
        (
            "DYLD_LIBRARY_PATH",
            &["runtime/maci64", "bin/maci64", "sys/os/maci64"],
        )
    } else {
        (
            "LD_LIBRARY_PATH",
            &[
                "runtime/glnxa64",
                "bin/glnxa64",
                "sys/os/glnxa64",
                "sys/opengl/lib/glnxa64",
            ],
        )
    };
    let mut paths: Vec<PathBuf> = dirs.iter().map(|dir| runtime.join(dir)).collect();
    paths.extend(std::env::var_os(var).iter().flat_map(std::env::split_paths));
    Ok(Some((var, std::env::join_paths(paths)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_solver_in_macos_app_bundle() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(solver_in(dir.path()), None);
        let inner = dir.path().join("mks_solver.app/Contents/MacOS");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("mks_solver"), "").unwrap();
        assert_eq!(solver_in(dir.path()), Some(inner.join("mks_solver")));
    }
}
