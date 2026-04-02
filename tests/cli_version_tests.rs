use std::process::Command;

#[test]
fn version_command_reports_release_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("version")
        .output()
        .expect("failed to run rune version");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Rune 0.2.0"));
    assert!(stdout.contains("release tag: v0.2.0"));
}
