//! The optimization problem file: models, design bounds, signals, weighted
//! objectives to minimize and hard constraints.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;

use super::expr::{self, Expr, is_identifier, is_reserved};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    models: BTreeMap<String, PathBuf>,
    #[serde(default)]
    optimizer: RawOptimizer,
    design: BTreeMap<String, [f64; 2]>,
    #[serde(default)]
    signals: BTreeMap<String, String>,
    #[serde(default)]
    objectives: BTreeMap<String, RawObjective>,
    #[serde(default)]
    constraints: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOptimizer {
    max_iterations: Option<u32>,
    max_evaluations: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawObjective {
    value: String,
    weight: Option<f64>,
    scale: Option<f64>,
}

/// An expression: the user's text and the text the solver evaluates.
#[derive(Debug, Clone, PartialEq)]
pub struct Formula {
    pub source: String,
    pub matlab: String,
}

/// A value to minimize. It counts as `value / scale`; without a scale the
/// solver uses |value| of the start design.
#[derive(Debug, Clone, PartialEq)]
pub struct Objective {
    pub name: String,
    pub value: Formula,
    pub weight: f64,
    pub scale: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Relation {
    AtMost,
    AtLeast,
}

/// `lhs <= rhs` or `lhs >= rhs`.
#[derive(Debug, Clone, PartialEq)]
pub struct Constraint {
    pub name: String,
    pub source: String,
    pub lhs: Formula,
    pub relation: Relation,
    pub rhs: Formula,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Design {
    pub name: String,
    pub lower: f64,
    pub upper: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    /// Model name and JSON path, the path made absolute from the problem file.
    pub models: BTreeMap<String, PathBuf>,
    pub max_iterations: Option<u32>,
    pub max_evaluations: Option<u32>,
    pub design: Vec<Design>,
    /// Signals in an order where each one comes after the signals it uses.
    pub signals: Vec<(String, Formula)>,
    pub objectives: Vec<Objective>,
    pub constraints: Vec<Constraint>,
}

/// The names a model JSON defines, for checking formulas.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ModelNames {
    /// Body name to the names of its points.
    pub bodies: BTreeMap<String, BTreeSet<String>>,
    pub joints: BTreeSet<String>,
}

impl ModelNames {
    pub fn from_json(model: &Value) -> Self {
        let keys = |value: &Value| -> BTreeSet<String> {
            value
                .as_object()
                .into_iter()
                .flat_map(|map| map.keys().cloned())
                .collect()
        };
        let bodies = model["Bodies"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, body)| {
                let points = body["Points_on_Body"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p.as_str().map(str::to_owned))
                    .collect();
                (name.clone(), points)
            })
            .collect();
        let mut joints = keys(&model["primary_joints"]);
        joints.extend(keys(&model["secondary_joints"]));
        Self { bodies, joints }
    }

    fn check_path(&self, model: &str, fields: &[&str]) -> Result<()> {
        match fields {
            ["bodies", body, rest @ ..] => {
                let Some(points) = self.bodies.get(*body) else {
                    bail!("model `{model}` has no body `{body}`");
                };
                if let ["points", point, ..] = rest
                    && !points.contains(*point)
                {
                    bail!("body `{body}` of model `{model}` has no point `{point}`");
                }
            }
            ["joints", joint, ..] if !self.joints.contains(*joint) => {
                bail!("model `{model}` has no joint `{joint}`");
            }
            _ => {}
        }
        Ok(())
    }
}

impl Problem {
    /// Reads and fully checks a problem file, including the names in its models.
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("cannot read problem file {}", path.display()))?;
        let base = path.parent().unwrap_or(Path::new("."));
        let problem = Self::parse(&text, base)
            .with_context(|| format!("invalid problem file {}", path.display()))?;
        let mut names = BTreeMap::new();
        for (name, file) in &problem.models {
            let text = fs::read_to_string(file)
                .with_context(|| format!("cannot read model `{name}`: {}", file.display()))?;
            let json: Value = serde_json::from_str(&text)
                .with_context(|| format!("model `{name}` is not JSON: {}", file.display()))?;
            names.insert(name.clone(), ModelNames::from_json(&json));
        }
        problem
            .check_models(&names)
            .with_context(|| format!("invalid problem file {}", path.display()))?;
        Ok(problem)
    }

    /// Checks everything that does not need the model files.
    pub fn parse(text: &str, base: &Path) -> Result<Self> {
        let raw: Raw = toml::from_str(text)?;
        for (key, value) in [
            ("max_iterations", raw.optimizer.max_iterations),
            ("max_evaluations", raw.optimizer.max_evaluations),
        ] {
            if value == Some(0) {
                bail!("[optimizer] {key} must be positive");
            }
        }

        if raw.models.is_empty() {
            bail!("[models] needs at least one model");
        }
        let mut taken: BTreeSet<&str> = BTreeSet::new();
        for name in raw.models.keys().chain(raw.signals.keys()) {
            if !is_identifier(name) {
                bail!(
                    "`{name}` is not a valid name: use letters, digits and `_`, starting with a letter"
                );
            }
            if is_reserved(name) {
                bail!("`{name}` is reserved");
            }
            if !taken.insert(name) {
                bail!("the name `{name}` is used for both a model and a signal");
            }
        }

        if raw.design.is_empty() {
            bail!("[design] needs at least one variable");
        }
        let mut design = Vec::new();
        for (name, [lower, upper]) in &raw.design {
            if !(lower.is_finite() && upper.is_finite() && lower < upper) {
                bail!("[design] {name}: need finite bounds with lower < upper");
            }
            design.push(Design {
                name: name.clone(),
                lower: *lower,
                upper: *upper,
            });
        }

        let models: BTreeSet<&str> = raw.models.keys().map(String::as_str).collect();
        let known = |name: &str| taken.contains(name);
        let signals = sorted_signals(&raw.signals, &models, &known)?;

        let formula = |what: &str, text: &str| -> Result<(Expr, Formula)> {
            let expr = expr::parse(text).with_context(|| format!("{what}: `{text}`"))?;
            expr.check_names(&known)
                .with_context(|| format!("{what}: `{text}`"))?;
            let matlab = expr.rewrite();
            Ok((
                expr,
                Formula {
                    source: text.to_owned(),
                    matlab,
                },
            ))
        };

        let mut objectives = Vec::new();
        for (name, o) in &raw.objectives {
            let what = format!("[objectives.{name}]");
            let weight = o.weight.unwrap_or(1.0);
            positive(weight, &format!("{what} weight"))?;
            if let Some(scale) = o.scale {
                positive(scale, &format!("{what} scale"))?;
            }
            let (_, value) = formula(&format!("{what} value"), &o.value)?;
            objectives.push(Objective {
                name: name.clone(),
                value,
                weight,
                scale: o.scale,
            });
        }
        if objectives.is_empty() {
            bail!("[objectives] needs at least one objective");
        }

        let mut constraints = Vec::new();
        for (name, text) in &raw.constraints {
            let what = format!("[constraints] {name}");
            let (lhs, relation, rhs) =
                split_constraint(text).with_context(|| format!("{what}: `{text}`"))?;
            constraints.push(Constraint {
                name: name.clone(),
                source: text.clone(),
                lhs: formula(&what, lhs)?.1,
                relation,
                rhs: formula(&what, rhs)?.1,
            });
        }

        let models = raw
            .models
            .into_iter()
            .map(|(name, file)| {
                (
                    name,
                    if file.is_absolute() {
                        file
                    } else {
                        base.join(file)
                    },
                )
            })
            .collect();
        Ok(Self {
            models,
            max_iterations: raw.optimizer.max_iterations,
            max_evaluations: raw.optimizer.max_evaluations,
            design,
            signals,
            objectives,
            constraints,
        })
    }

    /// Checks body, joint and point names in formulas against the models.
    pub fn check_models(&self, names: &BTreeMap<String, ModelNames>) -> Result<()> {
        let formulas = self
            .signals
            .iter()
            .map(|(name, f)| (format!("signal `{name}`"), f))
            .chain(
                self.objectives
                    .iter()
                    .map(|o| (format!("objective `{}`", o.name), &o.value)),
            )
            .chain(self.constraints.iter().flat_map(|c| {
                [
                    (format!("constraint `{}`", c.name), &c.lhs),
                    (format!("constraint `{}`", c.name), &c.rhs),
                ]
            }));
        for (what, formula) in formulas {
            let expr = expr::parse(&formula.source)?;
            for (root, fields) in expr.field_paths() {
                if let Some(model) = names.get(root) {
                    model
                        .check_path(root, &fields)
                        .with_context(|| format!("{what}: `{}`", formula.source))?;
                }
            }
        }
        Ok(())
    }
}

fn positive(value: f64, what: &str) -> Result<()> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        bail!("{what} must be positive")
    }
}

/// Splits `lhs <= rhs` or `lhs >= rhs`. Exactly one such operator is allowed.
fn split_constraint(text: &str) -> Result<(&str, Relation, &str)> {
    let found: Vec<_> = text
        .match_indices("<=")
        .chain(text.match_indices(">="))
        .collect();
    let [(at, op)] = found[..] else {
        bail!("need exactly one `<=` or `>=`");
    };
    let (lhs, rhs) = (&text[..at], &text[at + 2..]);
    let relation = if op == "<=" {
        Relation::AtMost
    } else {
        Relation::AtLeast
    };
    Ok((lhs, relation, rhs))
}

/// Orders signals so each follows the signals it uses; rejects unknown names and cycles.
fn sorted_signals(
    signals: &BTreeMap<String, String>,
    models: &BTreeSet<&str>,
    known: &impl Fn(&str) -> bool,
) -> Result<Vec<(String, Formula)>> {
    let mut parsed = BTreeMap::new();
    for (name, text) in signals {
        let what = format!("[signals] {name}: `{text}`");
        let expr = expr::parse(text).with_context(|| what.clone())?;
        expr.check_names(known).with_context(|| what.clone())?;
        parsed.insert(name.as_str(), expr);
    }

    let mut order: Vec<&str> = Vec::new();
    let mut state: BTreeMap<&str, bool> = BTreeMap::new(); // false: visiting, true: done
    fn visit<'a>(
        name: &'a str,
        parsed: &'a BTreeMap<&'a str, Expr>,
        models: &BTreeSet<&str>,
        state: &mut BTreeMap<&'a str, bool>,
        order: &mut Vec<&'a str>,
        path: &mut Vec<&'a str>,
    ) -> Result<()> {
        match state.get(name) {
            Some(true) => return Ok(()),
            Some(false) => {
                let start = path.iter().position(|n| *n == name).unwrap_or(0);
                let mut cycle = path[start..].to_vec();
                cycle.push(name);
                bail!(
                    "signals depend on each other in a cycle: {}",
                    cycle.join(" -> ")
                );
            }
            None => {}
        }
        state.insert(name, false);
        path.push(name);
        for used in parsed[name].references() {
            if parsed.contains_key(used) && !models.contains(used) {
                visit(
                    parsed.get_key_value(used).unwrap().0,
                    parsed,
                    models,
                    state,
                    order,
                    path,
                )?;
            }
        }
        path.pop();
        state.insert(name, true);
        order.push(name);
        Ok(())
    }
    for name in parsed.keys() {
        visit(
            name,
            &parsed,
            models,
            &mut state,
            &mut order,
            &mut Vec::new(),
        )?;
    }
    Ok(order
        .into_iter()
        .map(|name| {
            let formula = Formula {
                source: signals[name].clone(),
                matlab: parsed[name].rewrite(),
            };
            (name.to_owned(), formula)
        })
        .collect())
}

impl Problem {
    /// The problem as `mks_solver --optimize` reads it: rewritten expressions,
    /// absolute model paths. A missing scale is `null`.
    pub fn solver_json(&self) -> Value {
        let objectives: Vec<Value> = self
            .objectives
            .iter()
            .map(|o| {
                serde_json::json!({
                    "name": o.name,
                    "expr": o.value.matlab,
                    "weight": o.weight,
                    "scale": o.scale,
                })
            })
            .collect();
        let constraints: Vec<Value> = self
            .constraints
            .iter()
            .map(|c| {
                let relation = match c.relation {
                    Relation::AtMost => "<=",
                    Relation::AtLeast => ">=",
                };
                serde_json::json!({
                    "name": c.name,
                    "lhs": c.lhs.matlab,
                    "relation": relation,
                    "rhs": c.rhs.matlab,
                })
            })
            .collect();
        let mut json = serde_json::json!({
            "models": self.models,
            "design": self.design.iter().map(|d| serde_json::json!({
                "name": d.name, "lower": d.lower, "upper": d.upper,
            })).collect::<Vec<_>>(),
            "signals": self.signals.iter().map(|(name, f)| serde_json::json!({
                "name": name, "expr": f.matlab,
            })).collect::<Vec<_>>(),
            "objectives": objectives,
            "constraints": constraints,
        });
        for (key, value) in [
            ("max_iterations", self.max_iterations),
            ("max_evaluations", self.max_evaluations),
        ] {
            if let Some(value) = value {
                json[key] = value.into();
            }
        }
        json
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solver_json_has_the_schema_matlab_reads() {
        let problem = Problem::parse(VALID, Path::new("/base")).unwrap();
        let json = problem.solver_json();
        assert_eq!(json["models"]["pwt"], "/base/pwt.json");
        assert_eq!(json["max_iterations"], 50);
        assert!(json.get("max_evaluations").is_none());
        assert_eq!(json["design"][0]["name"], "HP_L_Z");
        let objectives = json["objectives"].as_array().unwrap();
        assert_eq!(objectives[0]["name"], "camber");
        assert_eq!(objectives[0]["scale"], 2.0);
        assert_eq!(objectives[1]["weight"], 2.0);
        assert!(objectives[1]["scale"].is_null());
        assert_eq!(json["signals"].as_array().unwrap().len(), 3);
    }

    const VALID: &str = r#"
[models]
pwt = "pwt.json"

[optimizer]
max_iterations = 50

[design]
HP_L_Z = [426.38, 446.38]
HP_X = [1, 2]

[signals]
epsilon = "rad2deg(atan(v.x ./ v.z))"
v = "pwt.characteristics.wheel_velocities.l_wc"
mid = "round(numel(v.z)/2)"

[objectives.wheel_motion]
value = "-epsilon(mid)"
weight = 2

[objectives.camber]
value = "abs(pwt.characteristics.toecamber.l_camber(end) + 20)"
scale = 2

[constraints]
c1 = "max(abs(gradient(epsilon))) <= 0.01"
"#;

    fn parse(text: &str) -> Result<Problem> {
        Problem::parse(text, Path::new("/dir"))
    }

    fn fail(text: &str) -> String {
        format!("{:#}", parse(text).unwrap_err())
    }

    fn replace(from: &str, to: &str) -> String {
        assert!(VALID.contains(from), "{from}");
        VALID.replace(from, to)
    }

    #[test]
    fn parses_the_valid_problem() {
        let p = parse(VALID).unwrap();
        assert_eq!(p.models["pwt"], Path::new("/dir/pwt.json"));
        assert_eq!(p.max_iterations, Some(50));
        assert_eq!(
            p.design[1],
            Design {
                name: "HP_X".into(),
                lower: 1.0,
                upper: 2.0
            }
        );
        let order: Vec<_> = p.signals.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(order, ["v", "epsilon", "mid"]);
        assert_eq!(
            p.signals[1].1.matlab,
            "rad2deg(atan((xyz(s.v,1)./xyz(s.v,3))))"
        );
        let camber = p.objectives.iter().find(|o| o.name == "camber").unwrap();
        assert_eq!((camber.weight, camber.scale), (1.0, Some(2.0)));
        let motion = p
            .objectives
            .iter()
            .find(|o| o.name == "wheel_motion")
            .unwrap();
        assert_eq!((motion.weight, motion.scale), (2.0, None));
        assert_eq!(p.constraints[0].relation, Relation::AtMost);
        assert_eq!(p.constraints[0].rhs.matlab, "0.01");
    }

    #[test]
    fn rejects_bad_structure() {
        assert!(fail(&replace("max_iterations = 50", "tolerance = 1")).contains("unknown field"));
        assert!(
            fail(&replace("max_iterations = 50", "max_iterations = 0"))
                .contains("must be positive")
        );
        assert!(fail(&replace("[426.38, 446.38]", "[5, 5]")).contains("lower < upper"));
        assert!(fail(&replace("pwt = \"pwt.json\"", "x = \"a.json\"")).contains("reserved"));
        assert!(fail(&replace("mid = ", "pwt = ")).contains("both a model and a signal"));
        assert!(fail(&replace("v = ", "max = ")).contains("reserved"));
        assert!(fail(&replace("v = ", "1v = ")).contains("not a valid name"));
    }

    #[test]
    fn rejects_bad_objectives() {
        assert!(fail(&replace("weight = 2", "weight = 0")).contains("weight must be positive"));
        assert!(fail(&replace("scale = 2", "scale = -1")).contains("scale must be positive"));
        assert!(fail(&replace("scale = 2", "target = 1")).contains("unknown field"));
    }

    #[test]
    fn rejects_bad_constraints() {
        assert!(fail(&replace("<= 0.01", "< 0.01")).contains("exactly one"));
        assert!(fail(&replace("<= 0.01", "<= 0.01 >= 0")).contains("exactly one"));
        assert!(fail(&replace("<= 0.01", "== 0.01")).contains("exactly one"));
        assert!(
            fail(&replace("max(abs(gradient(epsilon)))", "max(nope)"))
                .contains("unknown name `nope`")
        );
    }

    #[test]
    fn rejects_unknown_names_and_cycles() {
        assert!(
            fail(&replace("round(numel(v.z)/2)", "round(numel(w.z)/2)"))
                .contains("unknown name `w`")
        );
        assert!(fail(&replace("round(numel(v.z)/2)", "mid + 1")).contains("cycle: mid -> mid"));
        let cycle = replace("v = ", "a = \"b\"\nb = \"a\"\nv = ");
        assert!(fail(&cycle).contains("cycle: a -> b -> a"));
    }

    #[test]
    fn checks_body_joint_and_point_names_against_models() {
        let model = serde_json::json!({
            "Bodies": { "B1": { "Points_on_Body": ["P1"] } },
            "primary_joints": { "L1": {} },
            "secondary_joints": { "SCI": {} },
        });
        let names = BTreeMap::from([("pwt".to_owned(), ModelNames::from_json(&model))]);
        let problem = |value: &str| {
            parse(&replace("-epsilon(mid)", value))
                .unwrap()
                .check_models(&names)
        };
        assert!(problem("pwt.bodies.B1.points.P1.position.x(end)").is_ok());
        assert!(problem("pwt.joints.SCI.i.reaction.force.x(end)").is_ok());
        let error = |value: &str| format!("{:#}", problem(value).unwrap_err());
        assert!(error("pwt.bodies.B9.velocity.linear.x(end)").contains("no body `B9`"));
        assert!(error("pwt.bodies.B1.points.P9.position.x(end)").contains("no point `P9`"));
        assert!(error("pwt.joints.L9.i.position.x(end)").contains("no joint `L9`"));
    }
}
