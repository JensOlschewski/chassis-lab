#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod analyze;
mod convert;
mod inspect;
mod report;
mod session;
mod solve;
mod worker;

use std::{collections::BTreeMap, path::PathBuf};

use tauri::{AppHandle, Manager, State, ipc::Channel};

#[tauri::command]
async fn run_convert(
    app: AppHandle,
    request: convert::ConvertRequest,
) -> Result<worker::Outcome, String> {
    worker::run(&app, request.args()).await
}

#[tauri::command]
async fn run_inspect(
    app: AppHandle,
    request: inspect::InspectRequest,
) -> Result<worker::Outcome, String> {
    worker::run(&app, request.args()).await
}

#[tauri::command]
async fn run_analyze(
    app: AppHandle,
    request: analyze::AnalyzeRequest,
    on_line: Channel<String>,
) -> Result<worker::Outcome, String> {
    worker::run_streaming(&app, request.args()?, None, move |line| {
        if let worker::Line::Out(line) = line {
            let _ = on_line.send(line);
        }
    })
    .await
}

/// The hardpoints of a model file, in file order.
#[tauri::command]
fn model_hardpoints(path: PathBuf) -> Result<solve::Hardpoints, String> {
    solve::read_hardpoints(&path)
}

#[tauri::command]
fn read_design(path: PathBuf) -> Result<BTreeMap<String, f64>, String> {
    solve::read_design(&path)
}

/// Solves a model; `on_progress` gets the percent done as the solver reports it.
#[tauri::command]
async fn run_solve(
    app: AppHandle,
    request: solve::SolveRequest,
    cancel: State<'_, worker::Cancel>,
    on_progress: Channel<f64>,
) -> Result<worker::Outcome, String> {
    let prepared = request.case().prepare()?;
    let args = request.args(&prepared);
    let outcome = worker::run_streaming(&app, args, Some(cancel.inner().clone()), move |line| {
        if let worker::Line::Err(line) = line
            && let Some(percent) = solve::progress(&line)
        {
            let _ = on_progress.send(percent);
        }
    })
    .await;
    drop(prepared);
    outcome
}

#[tauri::command]
fn cancel_solve(cancel: State<'_, worker::Cancel>) {
    cancel.kill();
}

#[tauri::command]
fn solution_summary(path: PathBuf) -> Result<solve::Summary, String> {
    solve::summarize(&path)
}

/// Writes an exported chart image to the path the user chose in the save dialog.
#[tauri::command]
fn save_image(path: PathBuf, bytes: Vec<u8>) -> Result<(), String> {
    std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))
}

/// What a path the user typed points at.
#[derive(serde::Serialize, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
enum PathKind {
    Missing,
    /// A file, opened as signals.
    Signals,
    /// A folder with a `report.json`.
    Report,
    /// Any other folder.
    Folder,
}

fn kind_of(path: &std::path::Path) -> PathKind {
    if path.is_dir() {
        if path.join("report.json").is_file() {
            PathKind::Report
        } else {
            PathKind::Folder
        }
    } else if path.is_file() {
        PathKind::Signals
    } else {
        PathKind::Missing
    }
}

/// Tells Inspect whether a path is a report or signals, and Analyze whether a report folder exists.
#[tauri::command]
fn path_kind(path: PathBuf) -> PathKind {
    kind_of(&worker::clean_path(&path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_paths() {
        let dir = std::env::temp_dir().join(format!("clab-kind-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("run.h5");
        std::fs::write(&file, "").unwrap();
        assert_eq!(kind_of(&file), PathKind::Signals);
        assert_eq!(kind_of(&dir), PathKind::Folder);
        assert_eq!(kind_of(&dir.join("none")), PathKind::Missing);
        std::fs::write(dir.join("report.json"), "{}").unwrap();
        assert_eq!(kind_of(&dir), PathKind::Report);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// Reads one file of an opened report folder; see `report::read_file` for the allowed names.
#[tauri::command]
fn read_report_file(directory: PathBuf, name: String) -> Result<String, String> {
    report::read_file(&worker::clean_path(&directory), &name)
}

#[tauri::command]
async fn open_report_plots(directory: PathBuf) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let directory = worker::clean_path(&directory);
        if kind_of(&directory) != PathKind::Report {
            return Err(format!("{} is not a report folder", directory.display()));
        }
        let plots = directory.join("plots");
        let plots = plots
            .canonicalize()
            .map_err(|error| format!("{}: {error}", plots.display()))?;
        if !plots.is_dir() {
            return Err(format!("{} is not a folder", plots.display()));
        }
        #[cfg(target_os = "windows")]
        let opener = "explorer.exe";
        #[cfg(target_os = "macos")]
        let opener = "open";
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let opener = "xdg-open";
        let output = std::process::Command::new(opener)
            .arg(&plots)
            .output()
            .map_err(|error| format!("Could not open {}: {error}", plots.display()))?;
        if !output.status.success() {
            return Err(format!(
                "Could not open {} ({}): {}",
                plots.display(),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Inputs a report was computed from that are missing or no longer match; hashing may take a while, so it runs off the UI thread.
#[tauri::command]
async fn changed_report_inputs(directory: PathBuf) -> Result<Vec<report::ChangedInput>, String> {
    report::changed_inputs(&worker::clean_path(&directory))
}

#[tauri::command]
fn save_session(path: PathBuf, session: session::Session) -> Result<(), String> {
    session::save(&path, &session)
}

fn sessions_cache(app: &AppHandle) -> Result<PathBuf, String> {
    let cache = app
        .path()
        .app_cache_dir()
        .map_err(|error| error.to_string())?;
    Ok(cache.join("sessions"))
}

#[derive(serde::Serialize)]
struct LoadedSession {
    session: session::Session,
    embedded_paths: Vec<PathBuf>,
}

/// Embedded inputs and reports are extracted so the viewers can read them by path.
#[tauri::command]
fn load_session(app: AppHandle, path: PathBuf) -> Result<LoadedSession, String> {
    let root = sessions_cache(&app)?;
    let cache = session::cache_dir(&root, &path);
    let session = session::load(&path, &cache)?;
    let embedded_paths = session.embedded_paths(&cache);
    Ok(LoadedSession {
        session,
        embedded_paths,
    })
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(worker::Cancel::default())
        .setup(|app| {
            // Extracted inputs of earlier runs are unreachable now; a failure only wastes disk.
            let _ = session::clear_cache(&sessions_cache(app.handle())?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            run_convert,
            run_inspect,
            run_analyze,
            model_hardpoints,
            read_design,
            run_solve,
            cancel_solve,
            solution_summary,
            save_image,
            path_kind,
            read_report_file,
            open_report_plots,
            changed_report_inputs,
            save_session,
            load_session
        ])
        .run(tauri::generate_context!())
        .expect("failed to run ChassisLab desktop app");
}
