use std::{ffi::OsString, path::PathBuf};

use serde::Deserialize;

use crate::worker::clean_path;

#[derive(Deserialize)]
pub struct InspectRequest {
    file: PathBuf,
    /// Signals to read; empty lists the file's signals instead.
    signals: Vec<String>,
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
        }
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
