use std::{ffi::OsString, path::PathBuf};

use serde::Deserialize;

use crate::worker::clean_path;

/// One input is analyzed; more are compared, the first being the baseline.
#[derive(Deserialize)]
pub struct AnalyzeRequest {
    config: PathBuf,
    inputs: Vec<PathBuf>,
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
    fn no_input_is_rejected() {
        assert!(request(&[], None, false).args().is_err());
    }
}
