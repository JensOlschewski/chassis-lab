use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    io::BufWriter,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tempfile::NamedTempFile;

use crate::worker::clean_path;

/// `v_hat` holds 6 rows (3 linear, 3 angular velocity) per body.
const DOF_PER_BODY: usize = 6;

/// One hardpoint coordinate: a number, or the name of a design variable.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(untagged)]
enum Coordinate {
    Value(f64),
    Variable(String),
}

#[derive(Serialize, PartialEq, Debug)]
struct Point {
    name: String,
    coordinates: Vec<Coordinate>,
}

/// The hardpoints of a model, in file order.
#[derive(Serialize, PartialEq, Debug)]
pub struct Hardpoints {
    points: Vec<Point>,
}

impl Hardpoints {
    fn from_model(model: &Value) -> Result<Self, String> {
        let points = model["Hardpoints"]
            .as_object()
            .ok_or("the model has no Hardpoints")?
            .iter()
            .map(|(name, coordinates)| {
                let coordinates = Vec::deserialize(coordinates).map_err(|_| {
                    format!("hardpoint {name} is not a list of numbers and variable names")
                })?;
                Ok(Point {
                    name: name.clone(),
                    coordinates,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(Self { points })
    }

    fn variables(&self) -> BTreeSet<&str> {
        self.points
            .iter()
            .flat_map(|point| &point.coordinates)
            .filter_map(|coordinate| match coordinate {
                Coordinate::Variable(name) => Some(name.as_str()),
                Coordinate::Value(_) => None,
            })
            .collect()
    }
}

fn read_model(path: &Path) -> Result<Value, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("not a valid model JSON: {error}"))
}

pub fn read_hardpoints(path: &Path) -> Result<Hardpoints, String> {
    Hardpoints::from_model(&read_model(&clean_path(path))?)
}

/// Reads a design variable file: a flat JSON object of numbers.
pub fn read_design(path: &Path) -> Result<BTreeMap<String, f64>, String> {
    let path = clean_path(path);
    let text =
        std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("not a design file (an object of numbers): {error}"))
}

/// The model to solve: its design variable values, and new positions for hardpoints that are fixed numbers.
#[derive(Deserialize)]
pub struct Case {
    model: PathBuf,
    design: BTreeMap<String, f64>,
    #[serde(default)]
    points: BTreeMap<String, Vec<f64>>,
}

/// The files `clab solve` is given for a `Case`. Temporary files are deleted on drop.
pub struct Prepared {
    model: PathBuf,
    design: Option<PathBuf>,
    _files: Vec<NamedTempFile>,
}

fn finite(value: f64, what: &str) -> Result<f64, String> {
    value
        .is_finite()
        .then_some(value)
        .ok_or_else(|| format!("{what} needs a number"))
}

impl Case {
    /// Writes the design variable file, and a copy of the model with the adjusted points. The copy
    /// sits beside the model because the model's relative paths resolve from its directory.
    pub fn prepare(&self) -> Result<Prepared, String> {
        let model_path = clean_path(&self.model);
        let mut model = read_model(&model_path)?;
        let hardpoints = Hardpoints::from_model(&model)?;
        let mut files = Vec::new();

        let mut values = BTreeMap::new();
        for name in hardpoints.variables() {
            let value = self.design.get(name).copied().unwrap_or(f64::NAN);
            values.insert(name, finite(value, &format!("Design variable {name}"))?);
        }
        let mut design = None;
        if !values.is_empty() {
            let file = NamedTempFile::new().map_err(|error| error.to_string())?;
            serde_json::to_writer(BufWriter::new(&file), &values)
                .map_err(|error| error.to_string())?;
            design = Some(file.path().to_path_buf());
            files.push(file);
        }

        let mut solved = model_path;
        if !self.points.is_empty() {
            for (name, position) in &self.points {
                let current = hardpoints.points.iter().find(|point| point.name == *name);
                if current.is_none_or(|point| point.coordinates.len() != position.len()) {
                    return Err(format!("Hardpoint {name} does not match the model"));
                }
                let position = position
                    .iter()
                    .map(|value| finite(*value, &format!("Hardpoint {name}")))
                    .collect::<Result<Vec<_>, _>>()?;
                model["Hardpoints"][name] = position.into();
            }
            let directory = solved.parent().unwrap_or(Path::new("."));
            let file = tempfile::Builder::new()
                .prefix(".clab-")
                .suffix(".json")
                .tempfile_in(directory)
                .map_err(|error| format!("{}: {error}", directory.display()))?;
            serde_json::to_writer(BufWriter::new(&file), &model)
                .map_err(|error| error.to_string())?;
            solved = file.path().to_path_buf();
            files.push(file);
        }
        Ok(Prepared {
            model: solved,
            design,
            _files: files,
        })
    }
}

impl Prepared {
    fn args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> =
            vec!["solve".into(), (&self.model).into(), "--progress".into()];
        if let Some(design) = &self.design {
            args.extend(["--design".into(), design.into()]);
        }
        args
    }
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Include {
    Bodies,
    Connectors,
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

#[derive(Deserialize)]
pub struct SolveRequest {
    #[serde(flatten)]
    case: Case,
    output: PathBuf,
    include: Vec<Include>,
    force: bool,
}

impl SolveRequest {
    pub fn case(&self) -> &Case {
        &self.case
    }

    /// Builds the `clab solve` arguments. The model comes before `--json`, which takes an optional value.
    pub fn args(&self, prepared: &Prepared) -> Vec<OsString> {
        let mut args = prepared.args();
        args.extend(["--json".into(), clean_path(&self.output).into()]);
        if self.force {
            args.push("--force".into());
        }
        if !self.include.is_empty() {
            let names: Vec<_> = self.include.iter().map(|include| include.name()).collect();
            args.extend(["--include".into(), names.join(",").into()]);
        }
        args
    }
}

/// The percent done in a solver progress row (`step time percent`), `None` for any other line.
pub fn progress(line: &str) -> Option<f64> {
    let mut columns = line.split_whitespace();
    columns.next()?.parse::<u64>().ok()?;
    columns.next()?.parse::<f64>().ok()?;
    let percent = columns.next()?.parse().ok()?;
    columns.next().is_none().then_some(percent)
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Summary {
    steps: usize,
    end_time: f64,
    bodies: usize,
}

pub fn summarize(solution: &Path) -> Result<Summary, String> {
    #[derive(Deserialize)]
    struct Solution {
        time: Vec<f64>,
        v_hat: Vec<Vec<f64>>,
    }
    let solution = clean_path(solution);
    let text = std::fs::read_to_string(&solution)
        .map_err(|error| format!("{}: {error}", solution.display()))?;
    let Solution { time, v_hat } =
        serde_json::from_str(&text).map_err(|error| format!("not a solution file: {error}"))?;
    Ok(Summary {
        steps: time.len(),
        end_time: time.last().copied().unwrap_or_default(),
        bodies: v_hat.len() / DOF_PER_BODY,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL: &str = r#"{"Hardpoints": {
        "U": [1.0, 2.0, 3.0],
        "L": ["HP_L_X", "HP_L_Y", 5.0],
        "R": ["HP_L_X", "HP_R_Y", "HP_R_Z"]
    }}"#;

    fn case(design: &[(&str, f64)], points: &[(&str, [f64; 3])]) -> Case {
        let model = tempfile::tempdir().unwrap().keep().join("m.json");
        std::fs::write(&model, MODEL).unwrap();
        Case {
            model,
            design: design
                .iter()
                .map(|(name, value)| (name.to_string(), *value))
                .collect(),
            points: points
                .iter()
                .map(|(name, position)| (name.to_string(), position.to_vec()))
                .collect(),
        }
    }

    const ALL: [(&str, f64); 5] = [
        ("HP_L_X", 1.0),
        ("HP_L_Y", 2.0),
        ("HP_R_Y", 3.0),
        ("HP_R_Z", 4.0),
        ("OTHER", 9.0),
    ];

    #[test]
    fn hardpoints_keep_file_order_and_mark_variables() {
        let hardpoints = Hardpoints::from_model(&serde_json::from_str(MODEL).unwrap()).unwrap();
        let names: Vec<_> = hardpoints
            .points
            .iter()
            .map(|point| point.name.as_str())
            .collect();
        assert_eq!(names, ["U", "L", "R"]);
        assert_eq!(
            hardpoints.points[1].coordinates,
            [
                Coordinate::Variable("HP_L_X".into()),
                Coordinate::Variable("HP_L_Y".into()),
                Coordinate::Value(5.0)
            ]
        );
        assert_eq!(
            hardpoints.variables().into_iter().collect::<Vec<_>>(),
            ["HP_L_X", "HP_L_Y", "HP_R_Y", "HP_R_Z"]
        );
        assert!(Hardpoints::from_model(&serde_json::json!({})).is_err());
    }

    #[test]
    fn design_file_holds_the_model_variables_only() {
        let prepared = case(&ALL, &[]).prepare().unwrap();
        let design = prepared.design.as_ref().unwrap();
        let written: BTreeMap<String, f64> =
            serde_json::from_str(&std::fs::read_to_string(design).unwrap()).unwrap();
        assert_eq!(written.len(), 4);
        assert!(!written.contains_key("OTHER"));
    }

    #[test]
    fn design_file_needs_every_variable() {
        let error = case(&[("HP_L_X", 1.0)], &[]).prepare().err().unwrap();
        assert_eq!(error, "Design variable HP_L_Y needs a number");
        let error = case(&[("HP_L_X", f64::NAN)], &[]).prepare().err().unwrap();
        assert_eq!(error, "Design variable HP_L_X needs a number");
    }

    #[test]
    fn adjusted_points_go_into_a_copy_of_the_model_beside_it() {
        let case = case(&ALL, &[("U", [7.0, 8.0, 9.0])]);
        let prepared = case.prepare().unwrap();
        assert_ne!(prepared.model, case.model);
        assert_eq!(prepared.model.parent(), case.model.parent());
        let copy: Value =
            serde_json::from_str(&std::fs::read_to_string(&prepared.model).unwrap()).unwrap();
        assert_eq!(copy["Hardpoints"]["U"], serde_json::json!([7.0, 8.0, 9.0]));
        assert_eq!(copy["Hardpoints"]["L"][0], "HP_L_X");
        let copy_path = prepared.model.clone();
        drop(prepared);
        assert!(!copy_path.exists());
        assert!(
            std::fs::read_to_string(&case.model)
                .unwrap()
                .contains("1.0, 2.0, 3.0")
        );
    }

    #[test]
    fn adjusted_points_must_match_the_model() {
        let error = case(&ALL, &[("NOPE", [1.0, 2.0, 3.0])])
            .prepare()
            .err()
            .unwrap();
        assert_eq!(error, "Hardpoint NOPE does not match the model");
    }

    #[test]
    fn solve_arguments_put_the_model_before_json() {
        let request = SolveRequest {
            case: case(&ALL, &[]),
            output: PathBuf::from("/data/out.json"),
            include: vec![Include::Bodies, Include::Suspension],
            force: true,
        };
        let prepared = request.case.prepare().unwrap();
        let args = request.args(&prepared);
        let design = prepared.design.clone().unwrap();
        let expected: Vec<OsString> = vec![
            "solve".into(),
            request.case.model.clone().into(),
            "--progress".into(),
            "--design".into(),
            design.into(),
            "--json".into(),
            "/data/out.json".into(),
            "--force".into(),
            "--include".into(),
            "bodies,suspension".into(),
        ];
        assert_eq!(args, expected);
    }

    #[test]
    fn progress_reads_only_table_rows() {
        assert_eq!(
            progress("20           1.00e-01        40.00       "),
            Some(40.0)
        );
        assert_eq!(progress("Current Step  Current Time  Progress (%)"), None);
        assert_eq!(progress("------------ ------------- ------------"), None);
        assert_eq!(progress(""), None);
    }

    #[test]
    fn summary_counts_steps_and_bodies() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), r#"{"time": [0.0, 0.5, 1.0], "v_hat": [[0,0,0],[0,0,0],[0,0,0],[0,0,0],[0,0,0],[0,0,0]]}"#).unwrap();
        assert_eq!(
            summarize(file.path()).unwrap(),
            Summary {
                steps: 3,
                end_time: 1.0,
                bodies: 1
            }
        );
    }
}
