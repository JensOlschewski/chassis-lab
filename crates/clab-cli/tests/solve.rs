use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn clab(solver: &Path, runtime: &Path, args: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_clab"))
        .arg("solve")
        .args(args)
        .env("CLAB_MKS_SOLVER", solver)
        .env("CLAB_MATLAB_RUNTIME", runtime)
        .output()
        .unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[cfg(unix)]
fn fake_solver(dir: &Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("mks_solver");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn model(dir: &Path) -> PathBuf {
    let path = dir.join("model.json");
    std::fs::write(&path, "{}").unwrap();
    path
}

#[test]
fn reports_missing_solver() {
    let dir = tempfile::tempdir().unwrap();
    let output = clab(
        &dir.path().join("absent"),
        dir.path(),
        &[&model(dir.path())],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("CLAB_MKS_SOLVER does not point to a file"));
}

#[cfg(unix)]
#[test]
fn reports_missing_runtime() {
    let dir = tempfile::tempdir().unwrap();
    let solver = fake_solver(dir.path(), "exit 0");
    let output = clab(&solver, &dir.path().join("absent"), &[&model(dir.path())]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("CLAB_MATLAB_RUNTIME does not point to a directory"));
}

#[cfg(unix)]
#[test]
fn writes_solver_output_next_to_model() {
    let dir = tempfile::tempdir().unwrap();
    let solver = fake_solver(dir.path(), r#"echo "{\"cwd\":\"$(pwd)\"}" > "$2""#);
    let model = model(dir.path());
    let output = clab(&solver, dir.path(), &[&model]);
    assert!(output.status.success(), "{}", stderr(&output));
    let solution = dir.path().join("model-solution.json");
    let text = std::fs::read_to_string(solution).unwrap();
    let canonical = std::fs::canonicalize(dir.path()).unwrap();
    assert!(text.contains(canonical.to_str().unwrap()), "{text}");
}

#[cfg(unix)]
#[test]
fn refuses_to_replace_output_without_force() {
    let dir = tempfile::tempdir().unwrap();
    let solver = fake_solver(dir.path(), r#"echo '{}' > "$2""#);
    let model = model(dir.path());
    std::fs::write(dir.path().join("model-solution.json"), "old").unwrap();
    let output = clab(&solver, dir.path(), &[&model]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("--force"));
    let kept = std::fs::read_to_string(dir.path().join("model-solution.json")).unwrap();
    assert_eq!(kept, "old");
}

#[cfg(unix)]
#[test]
fn failed_solver_reports_stderr_and_leaves_no_output() {
    let dir = tempfile::tempdir().unwrap();
    let solver = fake_solver(dir.path(), "echo 'bad joint' >&2; exit 1");
    let output = clab(&solver, dir.path(), &[&model(dir.path())]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("bad joint"));
    assert!(!dir.path().join("model-solution.json").exists());
    let leftovers = std::fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(leftovers, 2, "only solver and model remain");
}

#[cfg(unix)]
#[test]
fn rejects_invalid_solver_json() {
    let dir = tempfile::tempdir().unwrap();
    let solver = fake_solver(dir.path(), r#"echo 'not json' > "$2""#);
    let output = clab(&solver, dir.path(), &[&model(dir.path())]);
    assert!(stderr(&output).contains("invalid JSON"));
    assert!(!dir.path().join("model-solution.json").exists());
}

/// Needs `dist/mks-solver/mks_solver` and a MATLAB Runtime; skipped otherwise.
#[test]
fn real_solver_solves_example_model() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let solver = root.join("dist/mks-solver/mks_solver");
    let runtime = [
        "/usr/local/MATLAB/MATLAB_Runtime/R2024b",
        "/usr/local/MATLAB/R2024b",
    ]
    .map(Path::new)
    .into_iter()
    .find(|path| path.is_dir());
    let (true, Some(runtime)) = (solver.is_file(), runtime) else {
        eprintln!("skipped: solver or MATLAB Runtime missing");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.json");
    let model = root.join("matlab/mlta-model-file-pwt-pos.json");
    let output = clab(&solver, runtime, &[&model, Path::new("-o"), &out]);
    assert!(output.status.success(), "{}", stderr(&output));
    let result: serde_json::Value = serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap();
    assert_eq!(result["time"].as_array().unwrap().len(), 101);
    assert_eq!(result["v_hat"].as_array().unwrap().len(), 84);
}
