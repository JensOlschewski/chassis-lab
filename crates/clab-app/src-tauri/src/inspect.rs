use std::{ffi::OsString, path::PathBuf};

use serde::Deserialize;

use crate::worker::clean_path;

#[derive(Deserialize)]
pub struct InspectRequest {
    file: PathBuf,
    /// Signals to read; empty lists the file's signals instead.
    signals: Vec<String>,
    /// Layout file for the solution viewer; ignored for HDF5.
    #[serde(default)]
    view: Option<PathBuf>,
}

impl InspectRequest {
    pub fn args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec!["inspect".into(), "--json".into()];
        if self.signals.is_empty() {
            args.push("--list".into());
        }
        for signal in &self.signals {
            args.extend(["--signal".into(), signal.into()]);
        }
        if let Some(view) = self
            .view
            .as_ref()
            .filter(|view| !view.as_os_str().is_empty())
        {
            args.extend(["--view".into(), clean_path(view).into()]);
        }
        args.push(clean_path(&self.file).into());
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(signals: &[&str]) -> InspectRequest {
        InspectRequest {
            file: PathBuf::from("\"/data/run.h5\" "),
            signals: signals.iter().map(|signal| signal.to_string()).collect(),
            view: None,
        }
    }

    #[test]
    fn passes_the_view_layout_before_the_file() {
        let mut request = request(&[]);
        request.view = Some(PathBuf::from("/data/layout.json"));
        let expected = [
            "inspect",
            "--json",
            "--list",
            "--view",
            "/data/layout.json",
            "/data/run.h5",
        ];
        assert_eq!(request.args(), expected.map(OsString::from));
    }

    #[test]
    fn lists_signals_when_none_requested() {
        let expected = ["inspect", "--json", "--list", "/data/run.h5"];
        assert_eq!(request(&[]).args(), expected.map(OsString::from));
    }

    #[test]
    fn reads_each_requested_signal() {
        let expected = [
            "inspect",
            "--json",
            "--signal",
            "time",
            "--signal",
            "lfwcx",
            "/data/run.h5",
        ];
        assert_eq!(
            request(&["time", "lfwcx"]).args(),
            expected.map(OsString::from)
        );
    }
}
