//! The Optimize pane: reads and writes `problem.toml` for the form and builds
//! the `clab optimize` call. Only the file structure is read here; `clab`
//! checks expressions and model names when it runs.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tempfile::{NamedTempFile, TempDir};

use crate::worker::clean_path;

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Optimizer {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_iterations: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_evaluations: Option<u32>,
}

impl Optimizer {
    fn is_empty(&self) -> bool {
        self.max_iterations.is_none() && self.max_evaluations.is_none()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Objective {
    value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    weight: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scale: Option<f64>,
}

/// The sections of `problem.toml`, in file order. Unknown keys are refused, so
/// a load never drops what a save would then lose.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Problem {
    models: IndexMap<String, PathBuf>,
    #[serde(default, skip_serializing_if = "Optimizer::is_empty")]
    optimizer: Optimizer,
    design: IndexMap<String, [f64; 2]>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    signals: IndexMap<String, String>,
    #[serde(default)]
    objectives: IndexMap<String, Objective>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    constraints: IndexMap<String, String>,
}

/// Reads a problem file; model paths are made absolute from its directory.
pub fn load(path: &Path) -> Result<Problem, String> {
    let path = clean_path(path);
    let text =
        std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut problem: Problem =
        toml::from_str(&text).map_err(|error| format!("not a problem file: {error}"))?;
    let base = path.parent().unwrap_or(Path::new("."));
    for model in problem.models.values_mut() {
        *model = base.join(&*model);
    }
    Ok(problem)
}

/// The problem as TOML, with model paths relative to `base` when they are inside it.
fn to_toml(problem: &Problem, base: &Path) -> Result<String, String> {
    let models = problem
        .models
        .iter()
        .map(|(name, path)| {
            let path = clean_path(path);
            // shortcut: a model outside `base` stays absolute; use `../` paths if files are shared.
            let path = path
                .strip_prefix(base)
                .map(Path::to_path_buf)
                .unwrap_or(path);
            (name.clone(), path)
        })
        .collect();
    let relative = Problem {
        models,
        ..problem.clone()
    };
    toml::to_string(&relative).map_err(|error| error.to_string())
}

/// Writes the problem file. An existing file is replaced only with `force`.
pub fn save(path: &Path, problem: &Problem, force: bool) -> Result<(), String> {
    let path = clean_path(path);
    if path.exists() && !force {
        return Err(format!("{} already exists", path.display()));
    }
    let text = to_toml(problem, path.parent().unwrap_or(Path::new(".")))?;
    std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))
}

/// Body names with their point names, and joint names, as formulas may use them.
#[derive(Serialize, Debug, PartialEq)]
pub struct ModelNames {
    bodies: BTreeMap<String, Vec<String>>,
    joints: Vec<String>,
}

pub fn model_names(path: &Path) -> Result<ModelNames, String> {
    let path = clean_path(path);
    let text =
        std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let model: Value =
        serde_json::from_str(&text).map_err(|error| format!("not a valid model JSON: {error}"))?;
    let bodies = model["Bodies"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, body)| {
            let points = body["Points_on_Body"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|point| point.as_str().map(str::to_owned))
                .collect();
            (name.clone(), points)
        })
        .collect();
    let joints = ["primary_joints", "secondary_joints"]
        .iter()
        .filter_map(|group| model[group].as_object())
        .flat_map(|joints| joints.keys().cloned())
        .collect();
    Ok(ModelNames { bodies, joints })
}

#[derive(Deserialize)]
pub struct OptimizeRequest {
    /// The problem file; `clab` runs from its directory.
    path: PathBuf,
    /// The adjusted form; without it the problem file runs as it is.
    problem: Option<Problem>,
    design: Option<PathBuf>,
    /// The result file; a dry run writes to a scratch file instead.
    output: Option<PathBuf>,
    force: bool,
    dry_run: bool,
}

/// The files `clab optimize` is given. Temporary ones are deleted on drop.
pub struct Prepared {
    problem: PathBuf,
    output: PathBuf,
    _problem_file: Option<NamedTempFile>,
    scratch: Option<TempDir>,
}

impl OptimizeRequest {
    /// An adjusted problem is written beside the problem file, so `clab` runs in
    /// the same directory.
    pub fn prepare(&self) -> Result<Prepared, String> {
        let path = clean_path(&self.path);
        let mut problem_file = None;
        let problem = match &self.problem {
            None => path,
            Some(problem) => {
                let directory = path.parent().unwrap_or(Path::new("."));
                let file = tempfile::Builder::new()
                    .prefix(".clab-")
                    .suffix(".toml")
                    .tempfile_in(directory)
                    .map_err(|error| format!("{}: {error}", directory.display()))?;
                let absolute = Problem {
                    models: (problem.models.iter())
                        .map(|(name, model)| (name.clone(), directory.join(clean_path(model))))
                        .collect(),
                    ..problem.clone()
                };
                let text = toml::to_string(&absolute).map_err(|error| error.to_string())?;
                std::fs::write(file.path(), text).map_err(|error| error.to_string())?;
                let temp = file.path().to_path_buf();
                problem_file = Some(file);
                temp
            }
        };
        let (scratch, output) = match (&self.output, self.dry_run) {
            (Some(output), false) => (None, clean_path(output)),
            (None, false) => return Err("Choose a result file".into()),
            (_, true) => {
                let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
                let output = dir.path().join("result.json");
                (Some(dir), output)
            }
        };
        Ok(Prepared {
            problem,
            output,
            _problem_file: problem_file,
            scratch,
        })
    }

    /// The problem comes before `--json`, which takes an optional value. The
    /// app asks about the midpoint start itself, so `clab` never prompts.
    pub fn args(&self, prepared: &Prepared) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec![
            "optimize".into(),
            (&prepared.problem).into(),
            "--progress".into(),
            "--yes".into(),
        ];
        if let Some(design) = &self.design {
            args.extend(["--design".into(), clean_path(design).into()]);
        }
        if self.dry_run {
            args.push("--dry-run".into());
        }
        args.extend(["--json".into(), (&prepared.output).into()]);
        if self.force && prepared.scratch.is_none() {
            args.push("--force".into());
        }
        args
    }
}

impl Prepared {
    pub fn result(&self) -> Result<Value, String> {
        let text = std::fs::read_to_string(&self.output)
            .map_err(|error| format!("{}: {error}", self.output.display()))?;
        serde_json::from_str(&text).map_err(|error| format!("not a result file: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../examples/matlab-examples/mlta-optimize-problem.toml")
    }

    #[test]
    fn example_loads_with_absolute_models_and_saves_relative() {
        let problem = load(&example()).unwrap();
        let model = &problem.models["pwt"];
        assert!(model.is_absolute() && model.ends_with("mlta-model-file-1e2-param.json"));
        assert_eq!(problem.design["HP_L_Z"], [426.38, 446.38]);
        assert_eq!(problem.optimizer.max_iterations, Some(100));
        let signals: Vec<_> = problem.signals.keys().map(String::as_str).collect();
        assert_eq!(signals[..2], ["v_wc", "r_wcwp"], "file order is kept");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.toml");
        let mut problem = problem;
        problem.models["pwt"] = dir.path().join("sub/m.json");
        save(&path, &problem, false).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("pwt = \"sub/m.json\""), "{text}");
        assert_eq!(load(&path).unwrap(), problem);
        assert!(
            save(&path, &problem, false)
                .unwrap_err()
                .ends_with("already exists")
        );
        save(&path, &problem, true).unwrap();
    }

    #[test]
    fn unknown_keys_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.toml");
        std::fs::write(
            &path,
            "[models]\nm = \"m.json\"\n[design]\nA = [0, 1]\n[extra]\n",
        )
        .unwrap();
        assert!(load(&path).unwrap_err().contains("extra"));
    }

    #[test]
    fn model_names_list_bodies_points_and_joints() {
        let model = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../examples/matlab-examples/mlta-model-file-1e2.json");
        let names = model_names(&model).unwrap();
        assert!(
            names
                .bodies
                .values()
                .any(|points| points.contains(&"RL".to_owned()))
        );
        assert!(!names.joints.is_empty());
    }

    fn request(problem: Option<Problem>, dry_run: bool) -> OptimizeRequest {
        let dir = tempfile::tempdir().unwrap().keep();
        OptimizeRequest {
            path: dir.join("p.toml"),
            problem,
            design: Some("start.json".into()),
            output: Some(dir.join("out.json")),
            force: true,
            dry_run,
        }
    }

    #[test]
    fn arguments_put_the_problem_before_json() {
        let request = request(None, false);
        let prepared = request.prepare().unwrap();
        let args = request.args(&prepared);
        let expected: Vec<OsString> = vec![
            "optimize".into(),
            request.path.clone().into(),
            "--progress".into(),
            "--yes".into(),
            "--design".into(),
            "start.json".into(),
            "--json".into(),
            request.output.clone().unwrap().into(),
            "--force".into(),
        ];
        assert_eq!(args, expected);
    }

    #[test]
    fn an_adjusted_problem_is_written_beside_the_problem_file() {
        let problem = load(&example()).unwrap();
        let request = request(Some(problem.clone()), false);
        let prepared = request.prepare().unwrap();
        assert_ne!(prepared.problem, request.path);
        assert_eq!(prepared.problem.parent(), request.path.parent());
        assert_eq!(
            load(&prepared.problem).unwrap(),
            problem,
            "models stay absolute"
        );
        let written = prepared.problem.clone();
        drop(prepared);
        assert!(!written.exists());
    }

    #[test]
    fn a_dry_run_writes_to_a_scratch_file() {
        let request = request(None, true);
        let prepared = request.prepare().unwrap();
        let args = request.args(&prepared);
        assert!(args.contains(&"--dry-run".into()));
        assert!(!args.contains(&"--force".into()));
        assert_ne!(Some(&prepared.output), request.output.as_ref());
        std::fs::write(&prepared.output, r#"{"J": 1}"#).unwrap();
        assert_eq!(prepared.result().unwrap()["J"], 1);
    }
}
