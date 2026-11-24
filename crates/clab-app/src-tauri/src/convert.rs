use std::{ffi::OsString, path::PathBuf};

use serde::Deserialize;

use crate::worker::clean_path;

#[derive(Deserialize)]
pub struct ConvertRequest {
    input: PathBuf,
    test: String,
    subtest: Option<String>,
    /// Setup code `b<0|1>e<0|1>f<0|1>r<0|1>`; clab requires it for knc.
    #[serde(default)]
    setup: Option<String>,
    output: Option<PathBuf>,
    force: bool,
    /// The user accepted the warnings of an earlier run.
    #[serde(default)]
    confirmed: bool,
}

impl ConvertRequest {
    /// Builds the `clab convert` arguments. Test/subtest combinations are
    /// validated by clab itself.
    pub fn args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> =
            vec!["convert".into(), "--test".into(), self.test.clone().into()];
        if let Some(subtest) = &self.subtest {
            args.extend(["--subtest".into(), subtest.clone().into()]);
        }
        if let Some(setup) = &self.setup {
            args.extend(["--setup".into(), setup.clone().into()]);
        }
        if let Some(output) = &self.output {
            args.extend(["--output".into(), clean_path(output).into()]);
        }
        if self.force {
            args.push("--force".into());
        }
        if self.confirmed {
            args.push("--yes".into());
        }
        args.push(clean_path(&self.input).into());
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ConvertRequest {
        ConvertRequest {
            input: PathBuf::from("\"/data/run.res\" "),
            test: "knc".into(),
            subtest: Some("vertical".into()),
            setup: Some("b1e0f1r1".into()),
            output: Some(PathBuf::from("/out")),
            force: true,
            confirmed: false,
        }
    }

    #[test]
    fn confirmed_adds_yes() {
        let request = ConvertRequest {
            confirmed: true,
            ..request()
        };
        let args = request.args();
        assert_eq!(args[args.len() - 2], OsString::from("--yes"));
    }

    #[test]
    fn builds_full_argument_list_with_cleaned_paths() {
        let expected = [
            "convert",
            "--test",
            "knc",
            "--subtest",
            "vertical",
            "--setup",
            "b1e0f1r1",
            "--output",
            "/out",
            "--force",
            "/data/run.res",
        ];
        assert_eq!(request().args(), expected.map(OsString::from));
    }

    #[test]
    fn omits_unset_options() {
        let request = ConvertRequest {
            subtest: None,
            setup: None,
            output: None,
            force: false,
            test: "svc".into(),
            ..request()
        };
        let expected = ["convert", "--test", "svc", "/data/run.res"];
        assert_eq!(request.args(), expected.map(OsString::from));
    }
}
