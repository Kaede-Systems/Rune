use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rune-cli-test-{stamp}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

#[test]
fn parse_errors_include_file_line_and_caret() {
    let dir = temp_dir();
    let path = dir.join("bad_parse.rn");
    fs::write(&path, "def main() -> i32\n    return 0\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("parse")
        .arg(&path)
        .output()
        .expect("failed to run rune parse");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("expected `:`"));
    assert!(stderr.contains("bad_parse.rn:1:"));
    assert!(stderr.contains("1 | def main() -> i32"));
    assert!(stderr.contains("^"));
}

#[test]
fn semantic_errors_include_file_line_and_caret() {
    let dir = temp_dir();
    let path = dir.join("bad_check.rn");
    fs::write(&path, "def main() -> i32:\n    return \"bad\"\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("check")
        .arg(&path)
        .output()
        .expect("failed to run rune check");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("return value expected `i32`, found `String`"));
    assert!(stderr.contains("bad_check.rn:2:"));
    assert!(stderr.contains("2 |     return \"bad\""));
    assert!(stderr.contains("^"));
}

#[test]
fn codegen_errors_include_file_line_and_caret() {
    let dir = temp_dir();
    let path = dir.join("bad_codegen.rn");
    fs::write(&path, "async def main() -> i32:\n    return 0\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("emit-asm")
        .arg(&path)
        .output()
        .expect("failed to run rune emit-asm");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("async functions are not supported by the current LLVM IR backend"));
    assert!(stderr.contains("bad_codegen.rn:1:"));
    assert!(stderr.contains("1 | async def main() -> i32:"));
    assert!(stderr.contains("^"));
}

#[test]
fn imported_module_semantic_errors_point_to_imported_file() {
    let dir = temp_dir();
    let lib_path = dir.join("math.rn");
    let main_path = dir.join("main.rn");
    fs::write(&lib_path, "def bad() -> i32:\n    return \"oops\"\n").unwrap();
    fs::write(
        &main_path,
        "import math\n\ndef main() -> i32:\n    return bad()\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("check")
        .arg(&main_path)
        .output()
        .expect("failed to run rune check");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("return value expected `i32`, found `String`"));
    assert!(stderr.contains("Traceback (most recent import last):"));
    assert!(stderr.contains("main.rn:1:1 imported `math`"));
    assert!(stderr.contains("math.rn:2:"));
    assert!(stderr.contains("2 |     return \"oops\""));
    assert!(stderr.contains("^"));
}

#[test]
fn imported_module_parse_errors_include_traceback_and_caret() {
    let dir = temp_dir();
    let lib_path = dir.join("math.rn");
    let main_path = dir.join("main.rn");
    fs::write(&lib_path, "def bad() -> i32\n    return 0\n").unwrap();
    fs::write(
        &main_path,
        "import math\n\ndef main() -> i32:\n    return 0\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("check")
        .arg(&main_path)
        .output()
        .expect("failed to run rune check");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Traceback (most recent import last):"));
    assert!(stderr.contains("main.rn:1:1 imported `math`"));
    assert!(stderr.contains("expected `:` after function signature"));
    assert!(stderr.contains("math.rn:1:"));
    assert!(stderr.contains("1 | def bad() -> i32"));
    assert!(stderr.contains("^"));
}

#[test]
fn missing_module_errors_point_to_import_site() {
    let dir = temp_dir();
    let main_path = dir.join("main.rn");
    fs::write(
        &main_path,
        "import missing_math\n\ndef main() -> i32:\n    return 0\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("check")
        .arg(&main_path)
        .output()
        .expect("failed to run rune check");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("module `missing_math` was not found"));
    assert!(stderr.contains("main.rn:1:"));
    assert!(stderr.contains("1 | import missing_math"));
    assert!(stderr.contains("^"));
}

#[test]
fn check_reports_multiple_semantic_errors_in_one_run() {
    let dir = temp_dir();
    let path = dir.join("multi_error.rn");
    fs::write(
        &path,
        "from fs import write_text\n\n\
         def main() -> i32:\n    let text = 42\n    println(write_text(\"out.txt\", text))\n    let b: i64 = 10\n    if b == 0:\n        return \"bad\"\n    return 0\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("check")
        .arg(&path)
        .output()
        .expect("failed to run rune check");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("function argument expected `String`, found `dynamic`"));
    assert!(stderr.contains("return value expected `i32`, found `String`"));
    assert!(stderr.contains("multi_error.rn:5:"));
    assert!(stderr.contains("multi_error.rn:8:"));
}
