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
