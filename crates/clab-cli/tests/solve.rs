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

fn json_args<'a>(model: &'a Path, out: &'a Path) -> [&'a Path; 3] {
    [model, Path::new("--json"), out]
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
fn writes_json_to_directory_and_runs_in_model_directory() {
    let dir = tempfile::tempdir().unwrap();
    let solver = fake_solver(dir.path(), r#"echo "{\"cwd\":\"$(pwd)\"}" > "$2""#);
    let model = model(dir.path());
    let out_dir = dir.path().join("out");
    std::fs::create_dir(&out_dir).unwrap();
    let output = clab(&solver, dir.path(), &json_args(&model, &out_dir));
    assert!(output.status.success(), "{}", stderr(&output));
    let solution = out_dir.join("model-solution.json");
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&solution).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o644);
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
    let target = dir.path().join("model-solution.json");
    let output = clab(&solver, dir.path(), &json_args(&model, &target));
    assert!(!output.status.success());
    assert!(stderr(&output).contains("--force"));
    let kept = std::fs::read_to_string(&target).unwrap();
    assert_eq!(kept, "old");
}

#[cfg(unix)]
#[test]
fn failed_solver_reports_stderr_and_leaves_no_output() {
    let dir = tempfile::tempdir().unwrap();
    let solver = fake_solver(dir.path(), "echo 'bad joint' >&2; exit 1");
    let model = model(dir.path());
    let target = dir.path().join("model-solution.json");
    let output = clab(&solver, dir.path(), &json_args(&model, &target));
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("bad joint"));
    assert!(!target.exists());
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
}

#[cfg(unix)]
#[test]
fn prints_text_without_json_and_passes_no_include() {
    let dir = tempfile::tempdir().unwrap();
    let solver = fake_solver(
        dir.path(),
        r#"echo '{"time":[0.0],"v_hat":[[1.0],[0.0],[0.0],[0.0],[0.0],[0.0]]}' > "$2""#,
    );
    let output = clab(&solver, dir.path(), &[&model(dir.path())]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("time 0.000000000000\nbody 1 velocity [1.000000000000"));
}

#[cfg(unix)]
#[test]
fn passes_include_list_to_solver() {
    let dir = tempfile::tempdir().unwrap();
    let solver = fake_solver(dir.path(), r#"printf '{"args":"%s"}' "$*" > "$2""#);
    let model = model(dir.path());
    let target = dir.path().join("o.json");
    let mut args = json_args(&model, &target).to_vec();
    args.extend([Path::new("--include"), Path::new("bodies,connectors")]);
    let output = clab(&solver, dir.path(), &args);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = std::fs::read_to_string(&target).unwrap();
    assert!(text.contains("--include bodies,connectors"), "{text}");
}

#[test]
fn include_needs_json_and_a_known_name() {
    let dir = tempfile::tempdir().unwrap();
    let model = model(dir.path());
    let run = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_clab"))
            .arg("solve")
            .arg(&model)
            .args(extra)
            .output()
            .unwrap()
    };
    assert!(!run(&["--include", "bodies"]).status.success());
    let bad = run(&["--json", "--include", "wheels"]);
    assert!(stderr(&bad).contains("wheels"));
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
    let examples = root.join("matlab/examples");
    let model = examples.join("mlta-model-file-pwt-pos-config-design.json");
    let design = examples.join("design-1e2.json");
    let args = [
        &model,
        Path::new("--design"),
        &design,
        Path::new("--json"),
        &out,
    ];
    let output = clab(&solver, runtime, &args);
    assert!(output.status.success(), "{}", stderr(&output));
    let result: serde_json::Value = serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap();
    assert_eq!(result["time"].as_array().unwrap().len(), 101);
    assert_eq!(result["v_hat"].as_array().unwrap().len(), 84);
}
