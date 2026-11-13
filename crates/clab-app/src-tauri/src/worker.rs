use std::{
    ffi::OsString,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};

use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
pub struct Outcome {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// A line the worker printed, with the stream it came from.
pub enum Line {
    Out(String),
    Err(String),
}

/// Stops the worker run it is passed to. Clones share one handle; the app has one long run at a time.
#[derive(Clone, Default)]
pub struct Cancel(Arc<Mutex<Option<u32>>>);

impl Cancel {
    fn set(&self, pid: Option<u32>) {
        *self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = pid;
    }

    /// Kills the running worker and everything it started, such as the MATLAB solver.
    pub fn kill(&self) {
        let pid = *self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(pid) = pid {
            kill_tree(pid);
        }
    }
}

/// The worker leads its own process group on unix (see `command`), so the group id is its pid.
#[cfg(unix)]
fn kill_tree(pid: u32) {
    let _ = Command::new("kill")
        .args(["-KILL", "--"])
        .arg(format!("-{pid}"))
        .status();
}

#[cfg(windows)]
fn kill_tree(pid: u32) {
    use std::os::windows::process::CommandExt;
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID"])
        .arg(pid.to_string())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Own process group, so `Cancel` can stop the worker together with its children.
        command.process_group(0);
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

/// Like `run_blocking`, but hands each line to `on_line` as clab prints it. Stderr is still
/// returned in the outcome; `stdout` is empty because the caller has already seen every line.
fn stream_blocking(
    clab: &Path,
    args: &[OsString],
    cancel: Option<&Cancel>,
    on_line: impl Fn(Line) + Sync,
) -> Result<Outcome, String> {
    let mut child = command(clab, args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start clab: {error}"))?;
    if let Some(cancel) = cancel {
        cancel.set(Some(child.id()));
    }
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    // Read on its own thread so a full stderr pipe cannot stall clab while we read stdout.
    let stderr = std::thread::scope(|scope| {
        let stderr = scope.spawn(|| {
            let mut text = String::new();
            for line in BufReader::new(stderr).split(b'\n').map_while(Result::ok) {
                let line = String::from_utf8_lossy(&line).into_owned();
                text.push_str(&line);
                text.push('\n');
                on_line(Line::Err(line));
            }
            text
        });
        for line in BufReader::new(stdout).split(b'\n').map_while(Result::ok) {
            on_line(Line::Out(String::from_utf8_lossy(&line).into_owned()));
        }
        stderr.join()
    });
    let status = child
        .wait()
        .map_err(|error| format!("failed to wait for clab: {error}"));
    if let Some(cancel) = cancel {
        cancel.set(None);
    }
    Ok(Outcome {
        exit_code: status?.code(),
        stdout: String::new(),
        stderr: stderr.map_err(|_| "stderr reader panicked")?,
    })
}

/// Runs the clab worker with `args` off the UI thread.
pub async fn run(app: &AppHandle, args: Vec<OsString>) -> Result<Outcome, String> {
    let clab = executable(app)?;
    tauri::async_runtime::spawn_blocking(move || run_blocking(&clab, &args))
        .await
        .map_err(|error| error.to_string())?
}

/// Runs the clab worker with `args` off the UI thread, passing its output lines to `on_line` live.
/// Killing the run through `cancel` ends it with no exit code.
pub async fn run_streaming(
    app: &AppHandle,
    args: Vec<OsString>,
    cancel: Option<Cancel>,
    on_line: impl Fn(Line) + Send + Sync + 'static,
) -> Result<Outcome, String> {
    let clab = executable(app)?;
    tauri::async_runtime::spawn_blocking(move || {
        stream_blocking(&clab, &args, cancel.as_ref(), on_line)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn sh(script: &str) -> Vec<OsString> {
        ["-c", script].map(OsString::from).to_vec()
    }

    #[test]
    fn streams_both_streams_and_collects_stderr() {
        let lines = Mutex::new(Vec::new());
        let outcome = stream_blocking(
            Path::new("sh"),
            &sh("echo one; echo oops >&2; printf two; exit 3"),
            None,
            |line| {
                lines.lock().unwrap().push(match line {
                    Line::Out(line) => format!("out {line}"),
                    Line::Err(line) => format!("err {line}"),
                })
            },
        )
        .unwrap();
        let mut lines = lines.into_inner().unwrap();
        lines.sort();
        assert_eq!(lines, ["err oops", "out one", "out two"]);
        assert_eq!(outcome.exit_code, Some(3));
        assert_eq!(outcome.stderr, "oops\n");
        assert!(outcome.stdout.is_empty());
    }

    #[test]
    fn cancel_kills_the_worker_and_its_children() {
        let cancel = Cancel::default();
        let stopper = cancel.clone();
        // The inner sh stands in for the solver that clab starts.
        let args = sh("sh -c 'sleep 30' & echo started; wait");
        let started = std::time::Instant::now();
        let outcome = stream_blocking(Path::new("sh"), &args, Some(&cancel), |_| stopper.kill());
        assert!(
            started.elapsed().as_secs() < 10,
            "cancel did not end the run"
        );
        assert_eq!(outcome.unwrap().exit_code, None);
    }
}
