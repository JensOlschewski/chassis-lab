//! `clab solve`: runs the compiled MATLAB multibody solver (`mks_solver`).
//!
//! The solver is a MATLAB Compiler executable. It needs the free MATLAB
//! Runtime of the release it was compiled with, so no MATLAB licence is needed.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use serde_json::Value;

use crate::matlab::{
    self, RUNTIME_RELEASE, file_path, find_runtime, find_solver, format_json, user_runtime_dir,
};
use crate::{inspect, solution};

const INSTALLER_ENV: &str = "CLAB_MATLAB_RUNTIME_INSTALLER";

#[derive(Debug, Subcommand)]
pub(crate) enum RuntimeCommand {
    /// Install the MATLAB Runtime the solver needs for this user.
    Install,
    /// Succeed when the solver and the MATLAB Runtime it needs are both found.
    Check,
}

#[derive(Debug, Args)]
pub(crate) struct SolveArgs {
    /// Multibody model JSON. Relative paths inside the model (bushing
    /// characteristics) resolve from the model's directory.
    input: PathBuf,
    /// Write the solution JSON to PATH: a file, or a directory that receives
    /// `<model>-solution.json`. Without PATH, the file goes to the current
    /// directory. Give the model before this flag.
    #[arg(long, value_name = "PATH", num_args = 0..=1)]
    json: Option<Option<PathBuf>>,
    /// Show the solved motion in the terminal. An optional view file (JSON)
    /// picks the bodies and points to draw; without it everything is drawn.
    #[arg(long, value_name = "FILE", num_args = 0..=1)]
    view: Option<Option<PathBuf>>,
    /// Design variable JSON applied to the model.
    #[arg(long, value_name = "FILE")]
    design: Option<PathBuf>,
    /// Replace an existing --json file.
    #[arg(long, requires = "json")]
    force: bool,
    /// Show solver progress on stderr.
    #[arg(long)]
    progress: bool,
}

pub(crate) fn solve(args: SolveArgs) -> Result<()> {
    let model = file_path(&args.input, "model")?;
    let design = args
        .design
        .as_deref()
        .map(|path| file_path(path, "design"))
        .transpose()?;
    let output = args
        .json
        .as_ref()
        .map(|path| matlab::output_path(path.as_deref(), &args.input, "solution"))
        .transpose()?;
    if let Some(output) = &output {
        matlab::check_output(output, &[(&model, "model")], args.force)?;
    }

    let layout = args
        .view
        .iter()
        .flatten()
        .map(|path| clab_core::view::ViewLayout::load(path))
        .next()
        .transpose()?;

    let scratch = matlab::scratch_file(output.as_deref())?;
    let mut solver_args = vec![model.as_path(), scratch.path()];
    if let Some(design) = &design {
        solver_args.extend([Path::new("--design"), design.as_path()]);
    }
    matlab::run_solver(
        &solver_args,
        model.parent().unwrap_or(Path::new(".")),
        args.progress,
    )?;

    let written = std::fs::File::open(scratch.path())?;
    let solution: Value = serde_json::from_reader(std::io::BufReader::new(written))
        .context("solver wrote invalid JSON")?;

    if let Some(output) = &output {
        let mut text = String::new();
        format_json(&solution, 0, &mut text);
        text.push('\n');
        std::fs::write(scratch.path(), text)
            .with_context(|| format!("cannot write {}", output.display()))?;
        scratch
            .persist(output)
            .with_context(|| format!("cannot write {}", output.display()))?;
        println!("{}", output.display());
    } else if args.view.is_none() {
        print!("{}", solution::render_text(&solution)?);
    }
    if args.view.is_some() {
        inspect::view_solution(&solution, layout.as_ref())?;
    }
    Ok(())
}

pub(crate) fn runtime(command: RuntimeCommand) -> Result<()> {
    match command {
        RuntimeCommand::Install => install_runtime(),
        RuntimeCommand::Check => find_solver().and(find_runtime()).map(drop),
    }
}

fn install_runtime() -> Result<()> {
    let installer = find_installer()?;
    let dir = user_runtime_dir().context("cannot find the user data directory")?;
    eprintln!(
        "installing MATLAB Runtime {RUNTIME_RELEASE} into {} (downloads about 1.5 GB)",
        dir.display()
    );
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let log = installer_log();
    let log_start = log
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .map_or(0, |m| m.len());
    let mut child = Command::new(&installer)
        .args([
            "-mode",
            "silent",
            "-agreeToLicense",
            "yes",
            "-destinationFolder",
        ])
        .arg(&dir)
        .spawn()
        .with_context(|| format!("cannot start {}", installer.display()))?;
    // The macOS installer logs "End - Successful" but its process never exits.
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if log
            .as_deref()
            .is_some_and(|p| log_reports_success(p, log_start))
        {
            child.kill()?;
            child.wait()?;
            break std::process::ExitStatus::default();
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    };
    if !status.success() {
        bail!("MATLAB Runtime installer failed: {status}");
    }
    if !dir.join(RUNTIME_RELEASE).is_dir() {
        bail!(
            "installer finished but installed nothing: {} is missing",
            dir.join(RUNTIME_RELEASE).display()
        );
    }
    Ok(())
}

fn installer_log() -> Option<PathBuf> {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok()?;
    Some(std::env::temp_dir().join(format!("mathworks_{user}.log")))
}

/// True when the installer log, past byte `from`, ends the run successfully.
fn log_reports_success(log: &Path, from: u64) -> bool {
    std::fs::read(log).is_ok_and(|bytes| {
        let new = bytes.get(from as usize..).unwrap_or_default();
        String::from_utf8_lossy(new).contains("End - Successful")
    })
}

fn find_installer() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(INSTALLER_ENV).map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
        bail!(
            "{INSTALLER_ENV} does not point to a file: {}",
            path.display()
        );
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| installer_in(exe.parent()?))
        .with_context(|| {
            format!("Runtime installer not found: install it next to clab or set {INSTALLER_ENV}")
        })
}

/// The installer `customInstaller` builds: a `.app` on macOS, `.install` on Linux.
fn installer_in(dir: &Path) -> Option<PathBuf> {
    [
        "mks_runtime.install",
        "mks_runtime_linux.install",
        "mks_runtime.exe",
        "mks_runtime.app/Contents/MacOS/setup",
    ]
    .iter()
    .map(|name| dir.join(name))
    .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_installer_in_macos_app_bundle() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(installer_in(dir.path()), None);
        let inner = dir.path().join("mks_runtime.app/Contents/MacOS");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("setup"), "").unwrap();
        assert_eq!(installer_in(dir.path()), Some(inner.join("setup")));
    }
}
