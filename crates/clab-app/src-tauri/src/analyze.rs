use std::{ffi::OsString, path::PathBuf};

use serde::Deserialize;

use crate::worker::clean_path;

#[derive(Deserialize)]
pub struct AnalyzeRequest {
    config: PathBuf,
    input: PathBuf,
    report: Option<PathBuf>,
    force: bool,
}

impl AnalyzeRequest {
    /// Builds the `clab analyze` arguments. Always `--json`: the app renders sections itself.
    pub fn args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec!["analyze".into(), "--json".into()];
        if let Some(report) = &self.report {
            args.extend(["--report".into(), clean_path(report).into()]);
            if self.force {
                args.push("--force".into());
            }
        }
        args.push(clean_path(&self.config).into());
        args.push(clean_path(&self.input).into());
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(report: Option<&str>, force: bool) -> AnalyzeRequest {
        AnalyzeRequest {
            config: PathBuf::from("\"/data/config.toml\" "),
            input: PathBuf::from("/data/run.toml"),
            report: report.map(PathBuf::from),
            force,
        }
    }

    #[test]
    fn minimal_request_asks_for_json() {
        let expected = ["analyze", "--json", "/data/config.toml", "/data/run.toml"];
        assert_eq!(request(None, false).args(), expected.map(OsString::from));
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
            request(Some("/out"), true).args(),
            expected.map(OsString::from)
        );
    }

    #[test]
    fn force_without_report_is_dropped() {
        assert!(!request(None, true).args().contains(&"--force".into()));
    }
}
