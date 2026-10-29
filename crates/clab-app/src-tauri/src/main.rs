#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod convert;
mod inspect;
mod worker;

use std::path::PathBuf;

use tauri::AppHandle;

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

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            run_convert,
            run_inspect,
            save_image
        ])
        .run(tauri::generate_context!())
        .expect("failed to run ChassisLab desktop app");
}
