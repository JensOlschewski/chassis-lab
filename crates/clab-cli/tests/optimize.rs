#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Command, Output};

const PROBLEM: &str = r#"
[models]
m = "model.json"
[design]
A = [0, 10]
[objectives.o]
value = "m.characteristics.x"
goal = "minimize"
"#;

const RESULT: &str = r#"{"design":{"A":5},"start_design":{"A":1},"objectives":{"name":"o","kind":"minimize","weight":1,"reference":3,"value":2,"d":-0.3,"grade":16,"rule":"reference"},"constraints":[],"grade":16,"start_grade":10,"fmincon":{"exitflag":1,"iterations":4,"evaluations":12,"failed_evaluations":0,"last_error":""}}"#;

struct Setup {
    dir: tempfile::TempDir,
    solver: PathBuf,
}

impl Setup {
    fn new(solver_body: &str) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("model.json"), "{}").unwrap();
        std::fs::write(dir.path().join("problem.toml"), PROBLEM).unwrap();
        std::fs::write(dir.path().join("start.json"), r#"{"A": 1}"#).unwrap();
        let solver = dir.path().join("mks_solver");
        std::fs::write(&solver, format!("#!/bin/sh\n{solver_body}\n")).unwrap();
        std::fs::set_permissions(&solver, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self { dir, solver }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_clab"))
            .arg("optimize")
            .args(args)
            .current_dir(self.dir.path())
            .env("CLAB_MKS_SOLVER", &self.solver)
            .env("CLAB_MATLAB_RUNTIME", self.dir.path())
            .output()
            .unwrap()
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn writes_result() -> String {
    format!("echo '{RESULT}' > \"$3\"")
}

#[test]
fn passes_the_problem_json_and_design_to_the_solver() {
    let setup = Setup::new(&format!(
        "[ \"$1\" = --optimize ] && [ \"$4\" = --design ] && cp \"$2\" args.json && {}",
        writes_result()
    ));
    let output = setup.run(&["problem.toml", "--design", "start.json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("grade 10.0000 -> 16.0000"), "{text}");
    let problem: serde_json::Value =
        serde_json::from_slice(&std::fs::read(setup.path("args.json")).unwrap()).unwrap();
    assert_eq!(problem["objectives"][0]["kind"], "minimize");
    assert_eq!(problem["design"][0]["name"], "A");
}

#[test]
fn dry_run_is_passed_on_and_conflicts_with_json() {
    let setup = Setup::new(&format!("[ \"$6\" = --dry-run ] && {}", writes_result()));
    let output = setup.run(&["problem.toml", "--design", "start.json", "--dry-run"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let output = setup.run(&[
        "problem.toml",
        "--design",
        "start.json",
        "--dry-run",
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn writes_the_optimized_design_and_refuses_to_replace_it() {
    let setup = Setup::new(&writes_result());
    let output = setup.run(&["problem.toml", "--design", "start.json", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let written = std::fs::read_to_string(setup.path("problem-optimized.json")).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&written).unwrap(),
        serde_json::json!({"A": 5})
    );
    let output = setup.run(&["problem.toml", "--design", "start.json", "--json"]);
    assert!(stderr(&output).contains("--force"));
    let output = setup.run(&[
        "problem.toml",
        "--design",
        "start.json",
        "--json",
        "start.json",
        "--force",
    ]);
    assert!(stderr(&output).contains("output is the design file"));
}

#[test]
fn json_before_the_problem_gets_a_clear_error() {
    let setup = Setup::new("exit 0");
    let output = setup.run(&["--design", "start.json", "--json", "problem.toml"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("give <PROBLEM> before --json"));
}

#[test]
fn needs_a_start_design() {
    let setup = Setup::new("exit 0");
    let output = setup.run(&["problem.toml"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("--design"));
}

#[test]
fn an_old_runtime_is_explained() {
    let setup = Setup::new("echo MCR:mclmcr:MCLMCR_Invalid_MATLAB_Runtime >&2; exit 1");
    let output = setup.run(&["problem.toml", "--design", "start.json"]);
    assert!(stderr(&output).contains("clab runtime install"));
}
