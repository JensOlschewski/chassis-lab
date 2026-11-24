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
fn refuses_to_replace_the_model_even_with_force() {
    let dir = tempfile::tempdir().unwrap();
    let solver = fake_solver(dir.path(), r#"echo '{}' > "$2""#);
    let model = model(dir.path());
    let mut args = json_args(&model, &model).to_vec();
    args.push(Path::new("--force"));
    let output = clab(&solver, dir.path(), &args);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("output is the model file"));
    assert_eq!(std::fs::read_to_string(&model).unwrap(), "{}");
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
fn prints_text_without_json() {
    let dir = tempfile::tempdir().unwrap();
    let solver = fake_solver(
        dir.path(),
        r#"echo '{"format":2,"time":[0.0],"bodies":{"Link":{"velocity":{"linear":[[1.0],[0.0],[0.0]],"angular":[[0.0],[0.0],[0.0]]}}}}' > "$2""#,
    );
    let output = clab(&solver, dir.path(), &[&model(dir.path())]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("time 0.000000000000\nbody 1 velocity [1.000000000000"));
}

/// Needs `dist/mks-solver/mks_solver` and a MATLAB Runtime; skipped otherwise.
#[test]
fn real_solver_solves_example_model() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let solver = std::env::var_os("CLAB_MKS_SOLVER")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("dist/mks-solver/mks_solver"));
    let runtime = std::env::var_os("CLAB_MATLAB_RUNTIME")
        .map(PathBuf::from)
        .into_iter()
        .chain(
            [
                "/usr/local/MATLAB/MATLAB_Runtime/R2024b",
                "/usr/local/MATLAB/R2024b",
            ]
            .map(PathBuf::from),
        )
        .find(|path| path.is_dir());
    let (true, Some(runtime)) = (solver.is_file(), runtime) else {
        eprintln!("skipped: solver or MATLAB Runtime missing");
        return;
    };
    let runtime = runtime.as_path();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out.json");
    let examples = root.join("matlab/examples/solve");
    let model = examples.join("mlta-model-file-1e2.json");
    let design = examples.join("mlta-design-file-1e2.json");
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
    assert_eq!(result["format"], 2);
    let wheelcarrier = &result["bodies"]["B2_Wheelcarrier"]["velocity"];
    assert_eq!(wheelcarrier["angular"].as_array().unwrap().len(), 3);
    assert_eq!(wheelcarrier["angular"][0].as_array().unwrap().len(), 101);

    // Bushing, force and time settings take design variables too. The copy sits beside the
    // model so that its relative characteristic paths keep working.
    let mut changed: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&model).unwrap()).unwrap();
    let bushing = &mut changed["primary_bushings"]["RL9"];
    bushing["active"] = "BUSH_ON".into();
    bushing["stiffness"][0] = "BUSH_KX".into();
    changed["sim_param"]["step_parameters"]["t_end"] = "T_END".into();
    changed["drives"]["relonomic_drives"]["Displacement Left Platform"]["signal"][2] =
        "STEER*TIME".into();
    let copy = tempfile::Builder::new()
        .suffix(".json")
        .tempfile_in(&examples)
        .unwrap();
    std::fs::write(copy.path(), changed.to_string()).unwrap();
    let mut variables: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&design).unwrap()).unwrap();
    variables["BUSH_ON"] = 1.into();
    variables["BUSH_KX"] = 500.into();
    variables["T_END"] = 10.into();
    variables["STEER"] = (-1).into();
    let variables_file = dir.path().join("design.json");
    std::fs::write(&variables_file, variables.to_string()).unwrap();
    let out = dir.path().join("changed.json");
    let args = [
        copy.path(),
        Path::new("--design"),
        &variables_file,
        Path::new("--json"),
        &out,
    ];
    let output = clab(&solver, runtime, &args);
    assert!(output.status.success(), "{}", stderr(&output));
    let result: serde_json::Value = serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap();
    assert_eq!(result["time"].as_array().unwrap().len(), 11);

    // Every joint is written; the bushing flag follows the joint ID, not the list position.
    let joints = result["joints"].as_object().unwrap();
    assert_eq!(joints.len(), 20);
    let flagged: Vec<_> = joints
        .iter()
        .filter(|(_, joint)| joint["bushing"] == true)
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(flagged, ["RL9"]);
    assert_eq!(
        joints["RU13"]["i"]["reaction"]["force"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}
