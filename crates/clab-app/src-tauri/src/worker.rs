use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
pub struct Outcome {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Packaged builds ship the relocatable clab runtime as the `clab-runtime` resource
/// directory. Development builds use the worker beside the app executable.
fn executable(app: &AppHandle) -> Result<PathBuf, String> {
    let name = if cfg!(windows) { "clab.exe" } else { "clab" };
    let mut candidates = Vec::new();
    if let Ok(resources) = app.path().resource_dir() {
        candidates.push(resources.join("clab-runtime/bin").join(name));
    }
    if let Some(directory) = std::env::current_exe()
        .ok()
        .and_then(|executable| executable.parent().map(Path::to_path_buf))
    {
        candidates.push(directory.join(name));
    }
    candidates
        .iter()
        .find(|path| path.is_file())
        .cloned()
        .ok_or_else(|| format!("clab worker not found; looked in {candidates:?}"))
}

fn run_blocking(clab: &Path, args: &[OsString]) -> Result<Outcome, String> {
    let mut command = Command::new(clab);
    command
        .args(args)
        // HDF5 cannot lock files on network shares such as \\wsl.localhost.
        .env("HDF5_USE_FILE_LOCKING", "FALSE");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // A GUI app must not flash a console window for each worker run.
        command.creation_flags(0x0800_0000);
    }
    let output = command
        .output()
        .map_err(|error| format!("failed to start clab: {error}"))?;
    Ok(Outcome {
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Runs the clab worker with `args` off the UI thread.
pub async fn run(app: &AppHandle, args: Vec<OsString>) -> Result<Outcome, String> {
    let clab = executable(app)?;
    tauri::async_runtime::spawn_blocking(move || run_blocking(&clab, &args))
        .await
        .map_err(|error| error.to_string())?
}
