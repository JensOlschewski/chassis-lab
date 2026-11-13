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

const BUSHING_GROUPS: [&str; 3] = [
    "primary_bushings",
    "secondary_bushings",
    "additional_bushings",
];
const DRIVE_GROUPS: [&str; 2] = ["force_torque_drives", "relonomic_drives"];
const STIFFNESS: [&str; 6] = ["k1", "k2", "k3", "k4", "k5", "k6"];
const DRIVE_COMPONENTS: [&str; 6] = ["x", "y", "z", "rx", "ry", "rz"];
/// Names in a drive expression that are not design variables.
const RESERVED: [&str; 3] = ["TIME", "time", "pi"];

#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Number,
    Switch,
    /// A text such as a characteristic file path.
    Text,
    /// A drive signal, evaluated by the solver over `TIME`.
    Expression,
}

/// One editable value of the model. It holds a literal `value`, or the name of a design `variable`.
#[derive(Serialize, PartialEq, Debug)]
struct Cell {
    /// JSON pointer to the value in the model file.
    pointer: String,
    column: usize,
    span: usize,
    kind: Kind,
    label: String,
    variable: Option<String>,
    value: Value,
    /// Shown but not editable.
    locked: bool,
}

impl Cell {
    fn new(pointer: String, kind: Kind, value: &Value) -> Option<Self> {
        let (variable, value) = match (kind, value) {
            (Kind::Number, Value::Number(_))
            | (Kind::Switch, Value::Bool(_))
            | (Kind::Expression, Value::String(_)) => (None, value.clone()),
            (Kind::Switch, Value::Number(number)) => {
                (None, Value::Bool(number.as_f64() != Some(0.0)))
            }
            (Kind::Text, Value::String(text)) if is_json_path(text) => (None, value.clone()),
            (_, Value::String(name)) => (Some(name.clone()), Value::Null),
            _ => return None,
        };
        Some(Self {
            pointer,
            column: 0,
            span: 1,
            kind,
            label: String::new(),
            variable,
            value,
            locked: false,
        })
    }
}

/// The solver reads a text ending in `.json` as a file path, any other text as a variable name.
fn is_json_path(text: &str) -> bool {
    Path::new(text)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
}

#[derive(Serialize, PartialEq, Debug)]
struct Row {
    name: String,
    cells: Vec<Cell>,
}

/// A table of related values, such as all bushings.
#[derive(Serialize, PartialEq, Debug)]
pub struct Section {
    id: &'static str,
    title: &'static str,
    columns: Vec<&'static str>,
    /// Whether a row's literal values are only editable after the user asks for it.
    lockable: bool,
    rows: Vec<Row>,
}

fn escape(name: &str) -> String {
    name.replace('~', "~0").replace('/', "~1")
}

/// The members of the object at `pointer`, in file order.
fn members<'a>(model: &'a Value, pointer: &str) -> impl Iterator<Item = &'a String> + use<'a> {
    model
        .pointer(pointer)
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|object| object.keys())
}

/// The cells of the value at `pointer`: one per array element, or one for a scalar.
/// `labels` name the cells; they start at `column`.
fn cells_at(model: &Value, pointer: &str, kind: Kind, column: usize, labels: &[&str]) -> Vec<Cell> {
    let Some(value) = model.pointer(pointer) else {
        return Vec::new();
    };
    let items: Vec<_> = match value {
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(index, item)| (format!("{pointer}/{index}"), item))
            .collect(),
        scalar => vec![(pointer.to_string(), scalar)],
    };
    items
        .into_iter()
        .enumerate()
        .filter_map(|(index, (pointer, item))| {
            let mut cell = Cell::new(pointer, kind, item)?;
            cell.column = column + index;
            cell.label = labels.get(index).unwrap_or(&"").to_string();
            Some(cell)
        })
        .collect()
}

/// Stiffness is a list of numbers, or a characteristic file (or a variable standing for one) over as many columns.
fn stiffness_cells(model: &Value, owner: &str, labels: &[&str]) -> Vec<Cell> {
    let pointer = format!("{owner}/stiffness");
    if model.pointer(&pointer).is_some_and(Value::is_string) {
        cells_at(model, &pointer, Kind::Text, 1, &["stiffness file"])
            .into_iter()
            .map(|cell| Cell {
                span: labels.len(),
                ..cell
            })
            .collect()
    } else {
        cells_at(model, &pointer, Kind::Number, 1, labels)
    }
}

fn bushing_row(model: &Value, group: &str, name: &str) -> Row {
    let owner = format!("/{group}/{}", escape(name));
    let mut cells = cells_at(
        model,
        &format!("{owner}/active"),
        Kind::Switch,
        0,
        &["active"],
    );
    cells.extend(stiffness_cells(model, &owner, &STIFFNESS));
    for (side, column) in [("i", 7), ("j", 10)] {
        let orientation = format!("{owner}/{side}/orientation");
        let found = [
            ("euler_angles", ["euler φ", "euler θ", "euler ψ"]),
            ("helpvec", ["help x", "help y", "help z"]),
        ]
        .into_iter()
        .find(|(key, _)| model.pointer(&format!("{orientation}/{key}")).is_some());
        if let Some((key, labels)) = found {
            let angles = cells_at(
                model,
                &format!("{orientation}/{key}"),
                Kind::Number,
                column,
                &labels,
            );
            // Only Euler angles are edited; a two-points orientation follows from the hardpoints.
            cells.extend(angles.into_iter().map(|cell| Cell {
                locked: key == "helpvec" && cell.variable.is_none(),
                ..cell
            }));
        }
    }
    Row {
        name: name.to_string(),
        cells,
    }
}

fn force_row(model: &Value, name: &str) -> Row {
    let owner = format!("/additional_forces/{}", escape(name));
    let mut cells = cells_at(
        model,
        &format!("{owner}/active"),
        Kind::Switch,
        0,
        &["active"],
    );
    cells.extend(stiffness_cells(model, &owner, &["stiffness"]));
    cells.extend(cells_at(
        model,
        &format!("{owner}/free_length"),
        Kind::Number,
        2,
        &["free length"],
    ));
    Row {
        name: name.to_string(),
        cells,
    }
}

/// Every value of the model that may be edited or replaced by a design variable. This is the list of
/// places `applydesignDicMapping.m` substitutes in; keep the two in step.
fn parameters(model: &Value) -> Result<Vec<Section>, String> {
    if !model["Hardpoints"].is_object() {
        return Err("the model has no Hardpoints".into());
    }
    let hardpoints = members(model, "/Hardpoints")
        .map(|name| Row {
            name: name.clone(),
            cells: cells_at(
                model,
                &format!("/Hardpoints/{}", escape(name)),
                Kind::Number,
                0,
                &["x", "y", "z"],
            ),
        })
        .collect();
    let bushings = BUSHING_GROUPS
        .iter()
        .flat_map(|group| {
            members(model, &format!("/{group}")).map(move |name| bushing_row(model, group, name))
        })
        .collect();
    let forces = members(model, "/additional_forces")
        .map(|name| force_row(model, name))
        .collect();
    let step = ["t_start", "t_end", "h"];
    let simulation = Row {
        name: "step".into(),
        cells: step
            .iter()
            .enumerate()
            .flat_map(|(column, key)| {
                cells_at(
                    model,
                    &format!("/sim_param/step_parameters/{key}"),
                    Kind::Number,
                    column,
                    &[key],
                )
            })
            .collect(),
    };
    let drives = DRIVE_GROUPS
        .iter()
        .flat_map(|group| {
            members(model, &format!("/drives/{group}")).map(move |name| Row {
                name: name.clone(),
                cells: cells_at(
                    model,
                    &format!("/drives/{group}/{}/signal", escape(name)),
                    Kind::Expression,
                    0,
                    &DRIVE_COMPONENTS,
                ),
            })
        })
        .collect();

    let mut bushing_columns = vec!["Active"];
    bushing_columns.extend(STIFFNESS);
    bushing_columns.extend(["i 1", "i 2", "i 3", "j 1", "j 2", "j 3"]);
    let sections = [
        (
            "hardpoints",
            "Hardpoints",
            vec!["X", "Y", "Z"],
            true,
            hardpoints,
        ),
        ("bushings", "Bushings", bushing_columns, false, bushings),
        (
            "forces",
            "Force Elements",
            vec!["Active", "Stiffness", "Free length"],
            false,
            forces,
        ),
        (
            "drives",
            "Drives and External Forces",
            DRIVE_COMPONENTS.to_vec(),
            false,
            drives,
        ),
        (
            "simulation",
            "Simulation Parameter",
            step.to_vec(),
            false,
            vec![simulation],
        ),
    ];
    Ok(sections
        .into_iter()
        .map(
            |(id, title, columns, lockable, rows): (_, _, _, _, Vec<Row>)| Section {
                id,
                title,
                columns,
                lockable,
                rows: rows
                    .into_iter()
                    .filter(|row| !row.cells.is_empty())
                    .collect(),
            },
        )
        .filter(|section| !section.rows.is_empty())
        .collect())
}

fn cells(sections: &[Section]) -> impl Iterator<Item = &Cell> {
    sections
        .iter()
        .flat_map(|section| &section.rows)
        .flat_map(|row| &row.cells)
}

/// Names in a drive expression that may be design variables: no numbers, function calls, `TIME` or `pi`.
fn identifiers(expression: &str) -> Vec<&str> {
    let word_char = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.';
    let mut names = Vec::new();
    let mut rest = expression;
    while let Some(start) = rest.find(word_char) {
        rest = &rest[start..];
        let (word, tail) = rest.split_at(rest.find(|c| !word_char(c)).unwrap_or(rest.len()));
        rest = tail;
        let is_name = word.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_');
        if is_name && !tail.trim_start().starts_with('(') && !RESERVED.contains(&word) {
            names.push(word);
        }
    }
    names
}

fn read_model(path: &Path) -> Result<Value, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("not a valid model JSON: {error}"))
}

pub fn read_parameters(path: &Path) -> Result<Vec<Section>, String> {
    parameters(&read_model(&clean_path(path))?)
}

/// The value of a design variable, as the design file holds it.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(untagged)]
pub enum DesignValue {
    Number(f64),
    Flag(bool),
    Text(String),
}

/// Reads a design variable file: a flat JSON object of numbers, switches and texts.
pub fn read_design(path: &Path) -> Result<BTreeMap<String, DesignValue>, String> {
    let path = clean_path(path);
    let text =
        std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| {
        format!("not a design file (an object of numbers, switches and texts): {error}")
    })
}

/// The model to solve: its design variable values, and new literal values by JSON pointer.
#[derive(Deserialize)]
pub struct Case {
    model: PathBuf,
    design: BTreeMap<String, DesignValue>,
    #[serde(default)]
    overrides: BTreeMap<String, Value>,
}

/// The files `clab solve` is given for a `Case`. Temporary files are deleted on drop.
pub struct Prepared {
    model: PathBuf,
    design: Option<PathBuf>,
    _files: Vec<NamedTempFile>,
}

impl Case {
    /// Writes the design variable file, and a copy of the model with the overrides applied. The copy
    /// sits beside the model because the model's relative paths resolve from its directory.
    pub fn prepare(&self) -> Result<Prepared, String> {
        let model_path = clean_path(&self.model);
        let mut model = read_model(&model_path)?;
        let known: BTreeSet<_> = cells(&parameters(&model)?)
            .map(|cell| cell.pointer.clone())
            .collect();
        for (pointer, value) in &self.overrides {
            let editable = known.contains(pointer) && (value.is_number() || value.is_string());
            match model.pointer_mut(pointer) {
                Some(slot) if editable => *slot = value.clone(),
                _ => return Err(format!("{pointer} does not match the model")),
            }
        }

        // Variables the overrides replaced are no longer needed.
        let sections = parameters(&model)?;
        let mut values = BTreeMap::new();
        for name in cells(&sections).filter_map(|cell| cell.variable.as_deref()) {
            let value = self
                .design
                .get(name)
                .ok_or_else(|| format!("Design variable {name} needs a value"))?;
            values.insert(name, value);
        }
        // Names in drive expressions are only a guess, so they count when the design has them.
        for cell in cells(&sections).filter(|cell| cell.kind == Kind::Expression) {
            for name in identifiers(cell.value.as_str().unwrap_or_default()) {
                if let Some(value) = self.design.get(name) {
                    values.insert(name, value);
                }
            }
        }

        let mut files = Vec::new();
        let mut design = None;
        if !values.is_empty() {
            let file = NamedTempFile::new().map_err(|error| error.to_string())?;
            serde_json::to_writer(BufWriter::new(&file), &values)
                .map_err(|error| error.to_string())?;
            design = Some(file.path().to_path_buf());
            files.push(file);
        }

        let mut solved = model_path;
        if !self.overrides.is_empty() {
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
    use serde_json::json;

    fn model() -> Value {
        json!({
            "Hardpoints": {
                "U": [1.0, 2.0, 3.0],
                "L": ["HP_L_X", "HP_L_Y", 5.0],
                "R": ["HP_L_X", "HP_R_Y", "HP_R_Z"]
            },
            "primary_bushings": {"B1": {
                "active": "B_ON",
                "stiffness": ["KX", 2, 3, 4, 5, 6],
                "i": {"orientation": {"method": "euler", "euler_angles": [0, 0, 0]}},
                "j": {"orientation": {"method": "two-points", "helpvec": [0, "HY", 0]}}
            }},
            "secondary_bushings": {"B2": {"active": 0, "stiffness": "K_FILE"}},
            "additional_forces": {
                "S1": {"active": 1, "stiffness": [30], "free_length": []},
                "S2": {"active": 0, "stiffness": "k/x.json", "free_length": [502.4]}
            },
            "sim_param": {"step_parameters": {"t_start": [0], "t_end": "T_END", "h": [1]}},
            "drives": {
                "force_torque_drives": {"f3": {"signal": ["AMP*sin(2*pi*TIME/30)+1e3", "~", "~", "~", "~", "~"]}},
                "relonomic_drives": {"b5": {"signal": ["~", "~", "STEER*TIME", "~", "~", "~"]}}
            }
        })
    }

    fn design() -> BTreeMap<String, DesignValue> {
        let mut design = BTreeMap::from(
            [
                ("HP_L_X", 1.0),
                ("HP_L_Y", 2.0),
                ("HP_R_Y", 3.0),
                ("HP_R_Z", 4.0),
                ("KX", 5.0),
                ("HY", 1.0),
                ("T_END", 10.0),
                ("AMP", 800.0),
                ("OTHER", 9.0),
            ]
            .map(|(name, value)| (name.to_string(), DesignValue::Number(value))),
        );
        design.insert("B_ON".into(), DesignValue::Flag(true));
        design.insert("K_FILE".into(), DesignValue::Text("k.json".into()));
        design
    }

    fn case(design: BTreeMap<String, DesignValue>, overrides: &[(&str, Value)]) -> Case {
        let model_path = tempfile::tempdir().unwrap().keep().join("m.json");
        std::fs::write(&model_path, model().to_string()).unwrap();
        Case {
            model: model_path,
            design,
            overrides: overrides
                .iter()
                .map(|(pointer, value)| (pointer.to_string(), value.clone()))
                .collect(),
        }
    }

    fn row<'a>(sections: &'a [Section], id: &str, name: &str) -> &'a Row {
        let section = sections.iter().find(|section| section.id == id).unwrap();
        section.rows.iter().find(|row| row.name == name).unwrap()
    }

    /// (column, span, variable or value) of each cell.
    fn layout(row: &Row) -> Vec<(usize, usize, String)> {
        row.cells
            .iter()
            .map(|cell| {
                let content = cell.variable.clone().unwrap_or(cell.value.to_string());
                (cell.column, cell.span, content)
            })
            .collect()
    }

    #[test]
    fn sections_follow_the_model() {
        let sections = parameters(&model()).unwrap();
        let ids: Vec<_> = sections.iter().map(|section| section.id).collect();
        assert_eq!(
            ids,
            ["hardpoints", "bushings", "forces", "drives", "simulation"]
        );
        let names: Vec<_> = sections[0]
            .rows
            .iter()
            .map(|row| row.name.as_str())
            .collect();
        assert_eq!(names, ["U", "L", "R"]);
        assert!(sections[0].lockable);
        assert!(parameters(&json!({})).is_err());

        assert_eq!(
            layout(row(&sections, "hardpoints", "L")),
            [
                (0, 1, "HP_L_X".into()),
                (1, 1, "HP_L_Y".into()),
                (2, 1, "5.0".into())
            ]
        );
        let bushing = layout(row(&sections, "bushings", "B1"));
        assert_eq!(bushing.len(), 13);
        assert_eq!(bushing[0], (0, 1, "B_ON".into()));
        assert_eq!(bushing[1], (1, 1, "KX".into()));
        assert_eq!(bushing[7], (7, 1, "0".into()));
        assert_eq!(bushing[11], (11, 1, "HY".into()));
        let cells = &row(&sections, "bushings", "B1").cells;
        assert!(!cells[7].locked, "euler angles are editable");
        assert!(cells[10].locked, "a literal helper vector is not");
        assert!(!cells[11].locked, "a variable still needs its value");
        assert_eq!(
            layout(row(&sections, "bushings", "B2")),
            [(0, 1, "false".into()), (1, 6, "K_FILE".into())]
        );
    }

    #[test]
    fn forces_simulation_and_drives_have_cells() {
        let sections = parameters(&model()).unwrap();
        assert_eq!(
            layout(row(&sections, "forces", "S1")),
            [(0, 1, "true".into()), (1, 1, "30".into())]
        );
        assert_eq!(
            layout(row(&sections, "forces", "S2")),
            [
                (0, 1, "false".into()),
                (1, 1, "\"k/x.json\"".into()),
                (2, 1, "502.4".into())
            ]
        );
        assert_eq!(
            layout(row(&sections, "simulation", "step")),
            [
                (0, 1, "0".into()),
                (1, 1, "T_END".into()),
                (2, 1, "1".into())
            ]
        );
        let drive = row(&sections, "drives", "f3");
        assert_eq!(drive.cells.len(), 6);
        assert_eq!(drive.cells[0].kind, Kind::Expression);
        assert_eq!(drive.cells[0].value, "AMP*sin(2*pi*TIME/30)+1e3");
    }

    #[test]
    fn example_model_lists_every_table() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../matlab/examples/mlta-model-file-pwt-neg-config-design.json");
        let model = read_model(&path).unwrap();
        let sections = parameters(&model).unwrap();
        let ids: Vec<_> = sections.iter().map(|section| section.id).collect();
        assert_eq!(
            ids,
            ["hardpoints", "bushings", "forces", "drives", "simulation"]
        );
        assert!(cells(&sections).all(|cell| model.pointer(&cell.pointer).is_some()));
    }

    #[test]
    fn expression_names_skip_numbers_calls_and_reserved_words() {
        assert_eq!(
            identifiers("AMP*sin(2*pi*TIME/30)+1e3 + K_2"),
            ["AMP", "K_2"]
        );
        assert_eq!(identifiers("~"), Vec::<&str>::new());
        assert_eq!(identifiers("-STEER*TIME"), ["STEER"]);
    }

    #[test]
    fn design_file_holds_what_the_model_uses() {
        let prepared = case(design(), &[]).prepare().unwrap();
        let written: BTreeMap<String, DesignValue> = serde_json::from_str(
            &std::fs::read_to_string(prepared.design.as_ref().unwrap()).unwrap(),
        )
        .unwrap();
        let names: Vec<_> = written.keys().map(String::as_str).collect();
        // OTHER is not in the model; STEER is a guess the design cannot back.
        assert_eq!(
            names,
            [
                "AMP", "B_ON", "HP_L_X", "HP_L_Y", "HP_R_Y", "HP_R_Z", "HY", "KX", "K_FILE",
                "T_END"
            ]
        );
        assert_eq!(written["B_ON"], DesignValue::Flag(true));
        assert_eq!(written["K_FILE"], DesignValue::Text("k.json".into()));
    }

    #[test]
    fn design_file_needs_every_variable() {
        let mut incomplete = design();
        incomplete.remove("HP_L_Y");
        let error = case(incomplete, &[]).prepare().err().unwrap();
        assert_eq!(error, "Design variable HP_L_Y needs a value");
    }

    #[test]
    fn overrides_go_into_a_copy_of_the_model_beside_it() {
        let case = case(
            design(),
            &[
                ("/Hardpoints/U/0", json!(7.5)),
                ("/additional_forces/S2/stiffness", json!("k/y.json")),
                ("/primary_bushings/B1/stiffness/0", json!(99)),
            ],
        );
        let prepared = case.prepare().unwrap();
        assert_ne!(prepared.model, case.model);
        assert_eq!(prepared.model.parent(), case.model.parent());
        let copy: Value =
            serde_json::from_str(&std::fs::read_to_string(&prepared.model).unwrap()).unwrap();
        assert_eq!(copy["Hardpoints"]["U"], json!([7.5, 2.0, 3.0]));
        assert_eq!(copy["additional_forces"]["S2"]["stiffness"], "k/y.json");
        assert_eq!(copy["Hardpoints"]["L"][0], "HP_L_X");
        let written = std::fs::read_to_string(prepared.design.as_ref().unwrap()).unwrap();
        assert!(
            !written.contains("\"KX\""),
            "an overridden variable is not needed"
        );
        let copy_path = prepared.model.clone();
        drop(prepared);
        assert!(!copy_path.exists());
        assert_eq!(read_model(&case.model).unwrap(), model());
    }

    #[test]
    fn overrides_must_match_the_model() {
        for (pointer, value) in [
            ("/Hardpoints/NOPE/0", json!(1.0)),
            ("/Bodies", json!(1.0)),
            ("/Hardpoints/U/0", json!([1.0])),
        ] {
            let error = case(design(), &[(pointer, value)]).prepare().err().unwrap();
            assert_eq!(error, format!("{pointer} does not match the model"));
        }
    }

    #[test]
    fn design_files_may_hold_switches_and_texts() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), r#"{"A": 1.5, "B": true, "C": "k.json"}"#).unwrap();
        let design = read_design(file.path()).unwrap();
        assert_eq!(design["A"], DesignValue::Number(1.5));
        assert_eq!(design["B"], DesignValue::Flag(true));
        assert_eq!(design["C"], DesignValue::Text("k.json".into()));
    }

    #[test]
    fn solve_arguments_put_the_model_before_json() {
        let request = SolveRequest {
            case: case(design(), &[]),
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
