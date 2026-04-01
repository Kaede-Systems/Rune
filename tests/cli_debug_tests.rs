use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rune-cli-debug-test-{stamp}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

#[test]
fn debug_command_emits_pipeline_and_runs_program() {
    let dir = temp_dir();
    let path = dir.join("debug_demo.rn");
    fs::write(
        &path,
        "def main() -> i32:\n    println(\"hello debug\")\n    return 0\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("debug")
        .arg(&path)
        .output()
        .expect("failed to run rune debug");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("== IR =="));
    assert!(stdout.contains("fn main:"));
    assert!(stdout.contains("== ASM =="));
    assert!(stdout.contains(".globl main"));
    assert!(stdout.contains("== Build =="));
    assert!(stdout.contains("debug_demo.debug.exe"));
    assert!(stdout.contains("== Run stdout =="));
    assert!(stdout.contains("hello debug"));
    assert!(stdout.contains("== Exit Code =="));
    assert!(stdout.contains("\n0\n"));
}

#[test]
fn debug_command_runs_relative_output_paths() {
    let dir = temp_dir();
    let path = dir.join("debug_rel.rn");
    fs::write(
        &path,
        "def main() -> i32:\n    println(\"relative ok\")\n    return 0\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .current_dir(&dir)
        .arg("debug")
        .arg("debug_rel.rn")
        .arg("-o")
        .arg("debug_rel.exe")
        .output()
        .expect("failed to run rune debug with relative output");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("debug_rel.exe"));
    assert!(stdout.contains("relative ok"));
    assert!(dir.join("debug_rel.exe").is_file());
}
