use std::{
    ffi::OsString,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
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

/// Pasted paths often carry whitespace or the quotes added by "Copy as path".
pub fn clean_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    PathBuf::from(text.trim().trim_matches('"').trim())
}

fn command(clab: &Path, args: &[OsString]) -> Command {
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
    command
}

fn run_blocking(clab: &Path, args: &[OsString]) -> Result<Outcome, String> {
    let output = command(clab, args)
        .output()
        .map_err(|error| format!("failed to start clab: {error}"))?;
    Ok(Outcome {
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Like `run_blocking`, but hands each stdout line to `on_line` as clab prints it.
/// The returned `stdout` is empty: the caller has already seen every line.
fn stream_blocking(
    clab: &Path,
    args: &[OsString],
    on_line: impl Fn(String),
) -> Result<Outcome, String> {
    let mut child = command(clab, args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start clab: {error}"))?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let mut stderr = child.stderr.take().expect("stderr is piped");
    // Drained on its own thread so a full stderr pipe cannot stall clab while we read stdout.
    let stderr = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    });
    for line in BufReader::new(stdout).split(b'\n') {
        let Ok(line) = line else { break };
        on_line(String::from_utf8_lossy(&line).into_owned());
    }
    let status = child
        .wait()
        .map_err(|error| format!("failed to wait for clab: {error}"))?;
    Ok(Outcome {
        exit_code: status.code(),
        stdout: String::new(),
        stderr: stderr.join().map_err(|_| "stderr reader panicked")?,
    })
}

/// Runs the clab worker with `args` off the UI thread.
pub async fn run(app: &AppHandle, args: Vec<OsString>) -> Result<Outcome, String> {
    let clab = executable(app)?;
    tauri::async_runtime::spawn_blocking(move || run_blocking(&clab, &args))
        .await
        .map_err(|error| error.to_string())?
}

/// Runs the clab worker with `args` off the UI thread, passing stdout lines to `on_line` live.
pub async fn run_streaming(
    app: &AppHandle,
    args: Vec<OsString>,
    on_line: impl Fn(String) + Send + 'static,
) -> Result<Outcome, String> {
    let clab = executable(app)?;
    tauri::async_runtime::spawn_blocking(move || stream_blocking(&clab, &args, on_line))
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn streams_stdout_lines_and_collects_stderr() {
        let lines = RefCell::new(Vec::new());
        let args = ["-c", "echo one; echo oops >&2; printf two; exit 3"].map(OsString::from);
        let outcome =
            stream_blocking(Path::new("sh"), &args, |line| lines.borrow_mut().push(line)).unwrap();
        assert_eq!(*lines.borrow(), ["one", "two"]);
        assert_eq!(outcome.exit_code, Some(3));
        assert_eq!(outcome.stderr, "oops\n");
        assert!(outcome.stdout.is_empty());
    }
}
