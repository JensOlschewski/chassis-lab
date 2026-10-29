#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod convert;
mod inspect;
mod session;
mod worker;

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

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

/// Writes an exported chart image to the path the user chose in the save dialog.
#[tauri::command]
fn save_image(path: PathBuf, bytes: Vec<u8>) -> Result<(), String> {
    std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))
}

#[tauri::command]
fn save_session(path: PathBuf, session: session::Session) -> Result<(), String> {
    session::save(&path, &session)
}

/// Embedded inputs are extracted into the app cache so they can be read by path.
#[tauri::command]
fn load_session(app: AppHandle, path: PathBuf) -> Result<session::Session, String> {
    let root = app
        .path()
        .app_cache_dir()
        .map_err(|error| error.to_string())?
        .join("sessions");
    session::load(&path, &session::cache_dir(&root, &path))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            run_convert,
            run_inspect,
            save_image,
            save_session,
            load_session
        ])
        .run(tauri::generate_context!())
        .expect("failed to run ChassisLab desktop app");
}
