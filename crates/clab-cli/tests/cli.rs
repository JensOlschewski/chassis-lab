use std::process::Command;

#[test]
fn emits_shell_completions() {
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let output = Command::new(env!("CARGO_BIN_EXE_clab"))
            .args(["completion", shell])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("clab"));
    }
}

#[test]
fn report_bundle_flags_are_report_only() {
    for command in ["analyze", "compare"] {
        let output = Command::new(env!("CARGO_BIN_EXE_clab"))
            .args([command, "--help"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("-r, --report"));
        assert!(!help.contains("--output"));
    }
}

#[test]
fn lists_canonical_signals_and_generates_toml_template() {
    let output = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args(["inspect", "--mapping", "knc", "--test", "vertical"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("knc:\n  vertical:\n"));
    assert!(text.contains("    front_left_wheel_center_x"));
    assert!(
        text.lines().any(|line| {
            line.starts_with("    front_left_wheel_center_x") && line.ends_with("mm")
        })
    );

    let output = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args(["inspect", "--mapping", "knc"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("knc:\n  front_left_travel"));
    assert!(text.contains("  vertical:\n"));

    let output = Command::new(env!("CARGO_BIN_EXE_clab"))
        .args(["inspect", "--mapping", "svc", "--template"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("[svc.signals.front_left_wheel_load]"));
    text.parse::<toml::Table>().unwrap();
}
