use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rune-cli-decompile-test-{stamp}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

#[test]
fn decompile_command_disassembles_built_binary() {
    let dir = temp_dir();
    let source_path = dir.join("demo.rn");
    let binary_path = dir.join("demo.exe");
    fs::write(
        &source_path,
        "def main() -> i32:\n    println(\"hello decompile\")\n    return 0\n",
    )
    .unwrap();

    let build = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("build")
        .arg(&source_path)
        .arg("-o")
        .arg(&binary_path)
        .output()
        .expect("failed to run rune build");
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("decompile")
        .arg(&binary_path)
        .arg("--format")
        .arg("asm")
        .output()
        .expect("failed to run rune decompile");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("file format"));
    assert!(stdout.contains("Disassembly of section"));
}

#[test]
fn decompile_command_rejects_unimplemented_c_output() {
    let dir = temp_dir();
    let source_path = dir.join("demo.rn");
    let binary_path = dir.join("demo.exe");
    fs::write(
        &source_path,
        "def main() -> i32:\n    println(\"hello decompile\")\n    return 0\n",
    )
    .unwrap();

    let build = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("build")
        .arg(&source_path)
        .arg("-o")
        .arg(&binary_path)
        .output()
        .expect("failed to run rune build");
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("decompile")
        .arg(&binary_path)
        .arg("--format")
        .arg("c")
        .output()
        .expect("failed to run rune decompile");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("generic binary-to-C decompilation is not implemented yet"));
}
