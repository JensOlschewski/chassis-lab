use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use serde::Deserialize;

#[derive(Deserialize)]
pub struct ConvertRequest {
    input: PathBuf,
    domain: String,
    test: Option<String>,
    output: Option<PathBuf>,
    force: bool,
}

/// Pasted paths often carry whitespace or the quotes added by "Copy as path".
fn clean_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    PathBuf::from(text.trim().trim_matches('"').trim())
}

impl ConvertRequest {
    /// Builds the `clab convert` arguments. Domain/test combinations are
    /// validated by clab itself.
    pub fn args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec![
            "convert".into(),
            "--domain".into(),
            self.domain.clone().into(),
        ];
        if let Some(test) = &self.test {
            args.extend(["--test".into(), test.clone().into()]);
        }
        if let Some(output) = &self.output {
            args.extend(["--output".into(), clean_path(output).into()]);
        }
        if self.force {
            args.push("--force".into());
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
            domain: "knc".into(),
            test: Some("vertical".into()),
            output: Some(PathBuf::from("/out")),
            force: true,
        }
    }

    #[test]
    fn builds_full_argument_list_with_cleaned_paths() {
        let expected = [
            "convert",
            "--domain",
            "knc",
            "--test",
            "vertical",
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
            test: None,
            output: None,
            force: false,
            domain: "svc".into(),
            ..request()
        };
        let expected = ["convert", "--domain", "svc", "/data/run.res"];
        assert_eq!(request.args(), expected.map(OsString::from));
    }
}
