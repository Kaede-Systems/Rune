use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rune::build::build_executable_llvm;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "rune-llvm-runtime-test-{}-{}-{}",
        std::process::id(),
        stamp,
        unique
    ));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

#[test]
fn llvm_backend_builds_and_runs_input_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_input_demo.rn");
    let exe_path = dir.join("llvm_input_demo.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    let line: String = input()\n    println(line)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm input program should build");

    let mut child = Command::new(&exe_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to start llvm-built executable");

    child
        .stdin
        .as_mut()
        .expect("stdin should be piped")
        .write_all(b"hello llvm\n")
        .expect("failed to write stdin");

    let output = child
        .wait_with_output()
        .expect("failed to collect llvm-built executable output");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "hello llvm\n");
}

#[test]
fn llvm_backend_builds_and_runs_panic_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_panic_demo.rn");
    let exe_path = dir.join("llvm_panic_demo.exe");

    fs::write(&source_path, "def main() -> i32:\n    panic(\"boom\")\n")
        .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm panic program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built panic executable");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n");
    assert!(stderr.contains("Rune panic: boom"));
    assert!(stderr.contains("panic in main"));
}

#[test]
fn llvm_backend_builds_and_runs_dynamic_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_dynamic_demo.rn");
    let exe_path = dir.join("llvm_dynamic_demo.exe");

    fs::write(
        &source_path,
        "def echo(value: dynamic) -> dynamic:\n    return value\n\n\
         def main() -> i32:\n    let value = 1\n    value = true\n    if value:\n        println(str(value))\n    println(echo(40 + 2))\n    println(str(echo(\"!\")) + \" ok\")\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm dynamic program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built dynamic executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "true\n42\n! ok\n");
}

#[test]
fn llvm_backend_builds_and_runs_string_int_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_string_int_demo.rn");
    let exe_path = dir.join("llvm_string_int_demo.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    println(int(\"123\"))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm string int program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built string int executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "123\n");
}
