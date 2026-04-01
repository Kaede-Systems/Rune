use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use rune::build::build_executable;

fn temp_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rune-runtime-dynamic-test-{stamp}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

#[test]
fn builds_and_runs_dynamic_add_program() {
    let dir = temp_dir();
    let source_path = dir.join("dynamic_add.rn");
    let exe_path = dir.join("dynamic_add.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    let value = 40\n    value = value + 2\n    println(value)\n    value = value + \"!\"\n    println(value)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("dynamic add program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "42\n42!\n");
}

#[test]
fn builds_and_runs_dynamic_comparison_program() {
    let dir = temp_dir();
    let source_path = dir.join("dynamic_cmp.rn");
    let exe_path = dir.join("dynamic_cmp.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    let value = 40\n    if value == 40:\n        println(\"eq\")\n    if value < 50:\n        println(\"lt\")\n    value = \"40\"\n    if value == 40:\n        println(\"string-eq\")\n    if value != 99:\n        println(\"ne\")\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("dynamic comparison program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "eq\nlt\nstring-eq\nne\n");
}

#[test]
fn builds_and_runs_dynamic_numeric_arithmetic_program() {
    let dir = temp_dir();
    let source_path = dir.join("dynamic_math.rn");
    let exe_path = dir.join("dynamic_math.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    let value = 10\n    value = value - 3\n    println(value)\n    value = value * 5\n    println(value)\n    value = value / 7\n    println(value)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("dynamic numeric arithmetic program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "7\n35\n5\n");
}

#[test]
fn builds_and_runs_boolean_operator_program() {
    let dir = temp_dir();
    let source_path = dir.join("dynamic_logic.rn");
    let exe_path = dir.join("dynamic_logic.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    let value = 1\n    value = true\n    if value and not false:\n        println(\"yes\")\n    if false or value:\n        println(\"or\")\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("dynamic logic program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "yes\nor\n");
}

#[test]
fn builds_and_runs_modulo_program() {
    let dir = temp_dir();
    let source_path = dir.join("modulo_demo.rn");
    let exe_path = dir.join("modulo_demo.exe");

    fs::write(
        &source_path,
        "def rem(a: i32, b: i32) -> i32:\n    return a % b\n\n\
         def main() -> i32:\n    let a: i32 = rem(10, 3)\n    println(a)\n    let value = 10\n    value = true\n    value = 10\n    value = value % 4\n    println(value)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("modulo program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "1\n2\n");
}

#[test]
fn builds_and_runs_panic_program() {
    let dir = temp_dir();
    let source_path = dir.join("panic_demo.rn");
    let exe_path = dir.join("panic_demo.exe");

    fs::write(&source_path, "def main() -> i32:\n    panic(\"boom\")\n")
        .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("panic program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Rune panic: boom"));
    assert!(stderr.contains("panic in main at line 2"));
}

#[test]
fn builds_and_runs_input_program() {
    let dir = temp_dir();
    let source_path = dir.join("input_demo.rn");
    let exe_path = dir.join("input_demo.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    println(input())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("input program should build");

    let mut child = Command::new(&exe_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to start built executable");

    child
        .stdin
        .as_mut()
        .expect("stdin should be piped")
        .write_all(b"hello rune\n")
        .expect("failed to write stdin");

    let output = child
        .wait_with_output()
        .expect("failed to collect built executable output");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "hello rune\n");
}

#[test]
fn builds_and_runs_stderr_output_program() {
    let dir = temp_dir();
    let source_path = dir.join("stderr_demo.rn");
    let exe_path = dir.join("stderr_demo.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    eprint(\"warn=\")\n    eprintln(42)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("stderr program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n");
    assert_eq!(stderr, "warn=42\n");
}

#[test]
fn builds_and_runs_bool_output_program() {
    let dir = temp_dir();
    let source_path = dir.join("bool_output_demo.rn");
    let exe_path = dir.join("bool_output_demo.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    println(true)\n    eprintln(false)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("bool output program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    let stderr = String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n");
    assert_eq!(stdout, "true\n");
    assert_eq!(stderr, "false\n");
}

#[test]
fn builds_and_runs_struct_program() {
    let dir = temp_dir();
    let source_path = dir.join("struct_demo.rn");
    let exe_path = dir.join("struct_demo.exe");

    fs::write(
        &source_path,
        "struct Point:\n    x: i32\n    y: i32\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    println(point.x)\n    println(point.y)\n    println(point.x + point.y)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("struct program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "20\n22\n42\n");
}

#[test]
fn builds_and_runs_struct_parameter_program() {
    let dir = temp_dir();
    let source_path = dir.join("struct_param_demo.rn");
    let exe_path = dir.join("struct_param_demo.exe");

    fs::write(
        &source_path,
        "struct Point:\n    x: i32\n    y: i32\n\n\
         def sum_point(point: Point) -> i32:\n    return point.x + point.y\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    println(sum_point(point))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("struct parameter program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "42\n");
}

#[test]
fn builds_and_runs_class_method_program() {
    let dir = temp_dir();
    let source_path = dir.join("class_method_demo.rn");
    let exe_path = dir.join("class_method_demo.exe");

    fs::write(
        &source_path,
        "class Point:\n    x: i32\n    y: i32\n    def sum(self) -> i32:\n        return self.x + self.y\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    println(point.sum())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("class method program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "42\n");
}

#[test]
fn builds_and_runs_fs_terminal_and_audio_program() {
    let dir = temp_dir();
    let source_path = dir.join("fs_terminal_audio_demo.rn");
    let exe_path = dir.join("fs_terminal_audio_demo.exe");
    let file_path = dir.join("note.txt");

    fs::write(
        &source_path,
        format!(
            "def main() -> i32:\n    println(__rune_builtin_fs_exists({:?}))\n    println(__rune_builtin_fs_write_string({:?}, \"hello rune\"))\n    println(__rune_builtin_fs_read_string({:?}))\n    __rune_builtin_terminal_clear()\n    __rune_builtin_terminal_move_to(1, 1)\n    __rune_builtin_terminal_hide_cursor()\n    __rune_builtin_terminal_set_title(\"Rune Test\")\n    __rune_builtin_terminal_show_cursor()\n    println(__rune_builtin_audio_bell())\n    return 0\n",
            file_path.to_string_lossy().to_string(),
            file_path.to_string_lossy().to_string(),
            file_path.to_string_lossy().to_string()
        ),
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("fs/terminal/audio program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert!(stdout.contains("false\ntrue\nhello rune\n"), "unexpected stdout: {stdout}");
    assert!(stdout.contains("true\n"), "unexpected stdout: {stdout}");
    let file_contents = fs::read_to_string(&file_path).expect("written file should exist");
    assert_eq!(file_contents, "hello rune");
}
