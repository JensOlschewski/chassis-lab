use std::{ffi::OsString, path::PathBuf};

use serde::Deserialize;

use crate::worker::clean_path;

/// One input is analyzed; more are compared, the first being the baseline.
#[derive(Deserialize)]
pub struct AnalyzeRequest {
    config: PathBuf,
    inputs: Vec<PathBuf>,
    /// Legend name per input, in order; empty keeps the default.
    #[serde(default)]
    names: Vec<String>,
    /// Apply configured tolerances; only a comparison has any.
    tolerances: bool,
    report: Option<PathBuf>,
    force: bool,
}

impl AnalyzeRequest {
    /// Builds the `clab analyze` or `clab compare` arguments. Always `--json`:
    /// the app renders sections itself.
    pub fn args(&self) -> Result<Vec<OsString>, String> {
        let compare = match self.inputs.len() {
            0 => return Err("Choose an input to analyze".into()),
            1 => false,
            _ => true,
        };
        let command = if compare { "compare" } else { "analyze" };
        let mut args: Vec<OsString> = vec![command.into(), "--json".into()];
        if compare && self.tolerances {
            args.push("--tolerances".into());
        }
        if self.names.iter().any(|name| !name.trim().is_empty()) {
            if self.names.len() != self.inputs.len() {
                return Err("Every input needs a name field".into());
            }
            // `--label=` keeps a name that starts with `-` from reading as a flag.
            args.extend(
                self.names
                    .iter()
                    .map(|name| format!("--label={name}").into()),
            );
        }
        if let Some(report) = &self.report {
            args.extend(["--report".into(), clean_path(report).into()]);
            if self.force {
                args.push("--force".into());
            }
        }
        args.push(clean_path(&self.config).into());
        args.extend(self.inputs.iter().map(|input| clean_path(input).into()));
        Ok(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(inputs: &[&str], report: Option<&str>, force: bool) -> AnalyzeRequest {
        AnalyzeRequest {
            config: PathBuf::from("\"/data/config.toml\" "),
            inputs: inputs.iter().map(PathBuf::from).collect(),
            names: Vec::new(),
            tolerances: true,
            report: report.map(PathBuf::from),
            force,
        }
    }

    #[test]
    fn one_input_is_analyzed_without_tolerances() {
        let expected = ["analyze", "--json", "/data/config.toml", "/data/run.toml"];
        assert_eq!(
            request(&["/data/run.toml"], None, false).args().unwrap(),
            expected.map(OsString::from)
        );
    }

    #[test]
    fn report_and_force_precede_the_paths() {
        let expected = [
            "analyze",
            "--json",
            "--report",
            "/out",
            "--force",
            "/data/config.toml",
            "/data/run.toml",
        ];
        assert_eq!(
            request(&["/data/run.toml"], Some("/out"), true)
                .args()
                .unwrap(),
            expected.map(OsString::from)
        );
    }

    #[test]
    fn force_without_report_is_dropped() {
        let args = request(&["/data/run.toml"], None, true).args().unwrap();
        assert!(!args.contains(&"--force".into()));
    }

    #[test]
    fn more_inputs_are_compared_against_the_first() {
        let expected = [
            "compare",
            "--json",
            "--tolerances",
            "--report",
            "/out",
            "/data/config.toml",
            "/data/base.h5",
            "/data/a.h5",
            "/data/b.h5",
        ];
        let inputs = ["/data/base.h5", "/data/a.h5", " \"/data/b.h5\""];
        assert_eq!(
            request(&inputs, Some("/out"), false).args().unwrap(),
            expected.map(OsString::from)
        );
    }

    #[test]
    fn names_are_passed_as_labels_in_input_order() {
        let mut named = request(&["/data/base.h5", "/data/a.h5"], None, false);
        named.names = vec!["foo".into(), "".into()];
        let args = named.args().unwrap();
        let labels: Vec<_> = args
            .iter()
            .filter(|arg| arg.to_string_lossy().starts_with("--label="))
            .collect();
        assert_eq!(labels, ["--label=foo", "--label="]);
        assert!(
            args.iter().position(|arg| arg == "--label=foo")
                < args.iter().position(|arg| arg == "/data/config.toml")
        );
    }

    #[test]
    fn blank_names_pass_no_labels() {
        let mut blank = request(&["/data/base.h5", "/data/a.h5"], None, false);
        blank.names = vec![" ".into(), "".into()];
        assert!(
            !blank
                .args()
                .unwrap()
                .iter()
                .any(|arg| arg.to_string_lossy().starts_with("--label"))
        );
    }

    #[test]
    fn a_single_input_passes_its_name() {
        let mut single = request(&["/data/run.toml"], None, false);
        single.names = vec!["foo".into()];
        assert!(single.args().unwrap().contains(&"--label=foo".into()));
    }

    #[test]
    fn mismatched_names_are_rejected() {
        let mut named = request(&["/data/base.h5", "/data/a.h5"], None, false);
        named.names = vec!["foo".into()];
        assert!(named.args().is_err());
    }

    #[test]
    fn no_input_is_rejected() {
        assert!(request(&[], None, false).args().is_err());
    }
}
