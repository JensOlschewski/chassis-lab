//! `clab solve`: runs the compiled MATLAB multibody solver (`mks_solver`).
//!
//! The solver is a MATLAB Compiler executable. It needs the free MATLAB
//! Runtime of the release it was compiled with, so no MATLAB licence is needed.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use serde_json::Value;

use crate::{solution, view};

const RUNTIME_RELEASE: &str = "R2024b";
const SOLVER_ENV: &str = "CLAB_MKS_SOLVER";
const RUNTIME_ENV: &str = "CLAB_MATLAB_RUNTIME";
const SOLVER_NAME: &str = if cfg!(windows) {
    "mks_solver.exe"
} else {
    "mks_solver"
};

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
    /// Optional solution content for --json, comma separated.
    #[arg(long, value_enum, value_delimiter = ',', requires = "json")]
    include: Vec<Include>,
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

#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
enum Include {
    /// Positions and velocities of bodies and their points.
    Bodies,
    /// Positions and reactions of joints and bushings.
    Connectors,
    /// Wheel, toe, camber, roll and rate characteristics.
    Suspension,
}

impl Include {
    fn name(self) -> &'static str {
        match self {
            Self::Bodies => "bodies",
            Self::Connectors => "connectors",
            Self::Suspension => "suspension",
        }
    }
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
        .map(|path| output_path(path.as_deref(), &args.input))
        .transpose()?;
    if let Some(output) = &output {
        if output.exists() && !args.force {
            bail!(
                "output exists: {} (use --force to replace it)",
                output.display()
            );
        }
        if !output.parent().is_some_and(Path::is_dir) {
            bail!("output directory does not exist: {}", output.display());
        }
    }

    let layout = args
        .view
        .iter()
        .flatten()
        .map(|path| view::ViewLayout::load(path))
        .next()
        .transpose()?;
    let mut include = args.include;
    if args.view.is_some() {
        let joints = layout.as_ref().is_none_or(view::ViewLayout::shows_joints);
        let needed = [Include::Bodies]
            .into_iter()
            .chain(joints.then_some(Include::Connectors));
        for needed in needed {
            if !include.contains(&needed) {
                include.push(needed);
            }
        }
    }

    let solver = find_solver()?;
    let runtime = find_runtime()?;

    // A file in the output directory can replace the output atomically on success.
    let scratch = match output.as_ref().and_then(|output| output.parent()) {
        Some(dir) => tempfile::NamedTempFile::new_in(dir),
        None => tempfile::NamedTempFile::new(),
    }
    .context("cannot create a scratch file")?;
    let mut command = Command::new(&solver);
    command
        .arg(&model)
        .arg(scratch.path())
        .current_dir(model.parent().unwrap_or(Path::new(".")));
    if let Some(design) = &design {
        command.arg("--design").arg(design);
    }
    if !include.is_empty() {
        let names: Vec<_> = include.iter().map(|item| item.name()).collect();
        command.arg("--include").arg(names.join(","));
    }
    if let Some(var) = library_path(&runtime)? {
        command.env(var.0, var.1);
    }
    if args.progress {
        command.env("MKS_PROGRESS", "1").stdout(std::io::stderr());
    }
    let result = command
        .output()
        .with_context(|| format!("cannot start {}", solver.display()))?;
    if !result.status.success() {
        bail!(
            "solver failed ({}): {}",
            result.status,
            String::from_utf8_lossy(&result.stderr).trim()
        );
    }

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
        let frames = view::frames_from_solution(&solution, layout.as_ref())?;
        view::run_viewer(frames)?;
    }
    Ok(())
}

/// `--json` without a value writes to the current directory; a directory
/// (existing, or written with a trailing separator) receives the default name.
fn output_path(requested: Option<&Path>, input: &Path) -> Result<PathBuf> {
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    let name = format!("{stem}-solution.json");
    let path = match requested {
        None => PathBuf::from(name),
        Some(path) if path.is_dir() || path.to_string_lossy().ends_with(['/', '\\']) => {
            path.join(name)
        }
        Some(path) => path.to_path_buf(),
    };
    absolute(&path)
}

/// Indents objects and arrays of containers; arrays of scalars stay on one line.
fn format_json(value: &Value, depth: usize, out: &mut String) {
    match value {
        Value::Array(items) if items.iter().any(|v| v.is_array() || v.is_object()) => {
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                out.push_str(&"  ".repeat(depth + 1));
                format_json(item, depth + 1, out);
                out.push_str(if index + 1 < items.len() { ",\n" } else { "\n" });
            }
            out.push_str(&"  ".repeat(depth));
            out.push(']');
        }
        Value::Object(map) if !map.is_empty() => {
            out.push_str("{\n");
            for (index, (key, item)) in map.iter().enumerate() {
                out.push_str(&"  ".repeat(depth + 1));
                out.push_str(&Value::from(key.as_str()).to_string());
                out.push_str(": ");
                format_json(item, depth + 1, out);
                out.push_str(if index + 1 < map.len() { ",\n" } else { "\n" });
            }
            out.push_str(&"  ".repeat(depth));
            out.push('}');
        }
        scalar_or_flat => out.push_str(&scalar_or_flat.to_string()),
    }
}

fn absolute(path: &Path) -> Result<PathBuf> {
    std::path::absolute(path).with_context(|| format!("invalid path: {}", path.display()))
}

fn file_path(path: &Path, what: &str) -> Result<PathBuf> {
    if !path.is_file() {
        bail!("{what} file does not exist: {}", path.display());
    }
    absolute(path)
}

fn find_solver() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(SOLVER_ENV).map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
        bail!("{SOLVER_ENV} does not point to a file: {}", path.display());
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(SOLVER_NAME)))
        .filter(|path| path.is_file())
        .with_context(|| {
            format!("solver not found: install {SOLVER_NAME} next to clab or set {SOLVER_ENV}")
        })
}

fn find_runtime() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(RUNTIME_ENV).map(PathBuf::from) {
        if path.is_dir() {
            return Ok(path);
        }
        bail!(
            "{RUNTIME_ENV} does not point to a directory: {}",
            path.display()
        );
    }
    let candidates: &[&str] = if cfg!(windows) {
        &["C:\\Program Files\\MATLAB\\MATLAB Runtime"]
    } else if cfg!(target_os = "macos") {
        &["/Applications/MATLAB/MATLAB_Runtime"]
    } else {
        &["/usr/local/MATLAB/MATLAB_Runtime", "/usr/local/MATLAB"]
    };
    candidates
        .iter()
        .map(|dir| Path::new(dir).join(RUNTIME_RELEASE))
        .find(|path| path.is_dir())
        .with_context(|| {
            format!("MATLAB Runtime {RUNTIME_RELEASE} not found: install it or set {RUNTIME_ENV}")
        })
}

/// Library search path variable the solver needs to find the Runtime.
/// Windows needs none: the Runtime installer adds itself to `PATH`.
fn library_path(runtime: &Path) -> Result<Option<(&'static str, OsString)>> {
    let (var, dirs): (&str, &[&str]) = if cfg!(windows) {
        return Ok(None);
    } else if cfg!(target_os = "macos") {
        (
            "DYLD_LIBRARY_PATH",
            &["runtime/maci64", "bin/maci64", "sys/os/maci64"],
        )
    } else {
        (
            "LD_LIBRARY_PATH",
            &[
                "runtime/glnxa64",
                "bin/glnxa64",
                "sys/os/glnxa64",
                "sys/opengl/lib/glnxa64",
            ],
        )
    };
    let mut paths: Vec<PathBuf> = dirs.iter().map(|dir| runtime.join(dir)).collect();
    paths.extend(std::env::var_os(var).iter().flat_map(std::env::split_paths));
    Ok(Some((var, std::env::join_paths(paths)?)))
}
