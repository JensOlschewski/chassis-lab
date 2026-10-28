#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use serde::Serialize;

#[derive(Serialize)]
struct AnalyzeOutcome {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// The worker is shipped beside the app executable.
fn clab_executable() -> Result<PathBuf, String> {
    let app = std::env::current_exe().map_err(|error| error.to_string())?;
    let name = if cfg!(windows) { "clab.exe" } else { "clab" };
    let path = app
        .parent()
        .ok_or("application directory cannot be determined")?
        .join(name);
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("clab worker not found: {}", path.display()))
    }
}

fn analyze(config: &Path, input: &Path, report: &Path) -> Result<AnalyzeOutcome, String> {
    for (label, path) in [("configuration", config), ("input", input)] {
        if !path.is_file() {
            return Err(format!("{label} file not found: {}", path.display()));
        }
    }
    let output = Command::new(clab_executable()?)
        .arg("analyze")
        .arg(config)
        .arg(input)
        .arg("--report")
        .arg(report)
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
    config: PathBuf,
    input: PathBuf,
    report: PathBuf,
) -> Result<AnalyzeOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || analyze(&config, &input, &report))
        .await
        .map_err(|error| error.to_string())?
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![run_analyze])
        .run(tauri::generate_context!())
        .expect("failed to run ChassisLab desktop app");
}
