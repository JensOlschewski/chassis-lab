#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod convert;
mod worker;

use tauri::AppHandle;

#[tauri::command]
async fn run_convert(
    app: AppHandle,
    request: convert::ConvertRequest,
) -> Result<worker::Outcome, String> {
    worker::run(&app, request.args()).await
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![run_convert])
        .run(tauri::generate_context!())
        .expect("failed to run ChassisLab desktop app");
}
