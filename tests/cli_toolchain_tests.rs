use std::process::Command;

#[test]
fn toolchain_command_reports_packaged_tools() {
    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("toolchain")
        .output()
        .expect("failed to run rune toolchain");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Bundled LLVM tools:"));
    assert!(stdout.contains("llc"));
    assert!(stdout.contains("lld-link"));
    assert!(stdout.contains("wasm-ld"));
}
