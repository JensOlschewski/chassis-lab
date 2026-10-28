#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
struct AnalyzeOutcome {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Packaged builds ship the relocatable clab runtime as the `clab-runtime` resource
/// directory. Development builds use the worker beside the app executable.
fn clab_executable(app: &AppHandle) -> Result<PathBuf, String> {
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

fn analyze(
    clab: &Path,
    config: &Path,
    input: &Path,
    report: &Path,
) -> Result<AnalyzeOutcome, String> {
    for (label, path) in [("configuration", config), ("input", input)] {
        if !path.is_file() {
            return Err(format!("{label} file not found: {}", path.display()));
        }
    }
    let mut command = Command::new(clab);
    command
        .arg("analyze")
        .arg(config)
        .arg(input)
        .arg("--report")
        .arg(report);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // A GUI app must not flash a console window for each worker run.
        command.creation_flags(0x0800_0000);
    }
    let output = command
        .output()
        .map_err(|error| format!("failed to start clab: {error}"))?;
    Ok(AnalyzeOutcome {
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

#[tauri::command]
async fn run_analyze(
    app: AppHandle,
    config: PathBuf,
    input: PathBuf,
    report: PathBuf,
) -> Result<AnalyzeOutcome, String> {
    let clab = clab_executable(&app)?;
    tauri::async_runtime::spawn_blocking(move || analyze(&clab, &config, &input, &report))
        .await
        .map_err(|error| error.to_string())?
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![run_analyze])
        .run(tauri::generate_context!())
        .expect("failed to run ChassisLab desktop app");
}
