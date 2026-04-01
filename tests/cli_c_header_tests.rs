use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir() -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rune-cli-c-header-test-{stamp}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

#[test]
fn emit_c_header_command_writes_header_file() {
    let dir = temp_dir();
    let source_path = dir.join("lib.rn");
    let header_path = dir.join("lib_generated.h");

    fs::write(
        &source_path,
        "def add(a: i32, b: i32) -> i32:\n    return a + b\n",
    )
    .expect("failed to write rune source");

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("emit-c-header")
        .arg(&source_path)
        .arg("-o")
        .arg(&header_path)
        .output()
        .expect("failed to run rune emit-c-header");

    assert!(output.status.success());
    let header = fs::read_to_string(&header_path).expect("failed to read generated header");
    assert!(header.contains("int32_t add(int32_t a, int32_t b);"));
}
