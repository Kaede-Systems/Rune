use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rune::llvm_backend::{emit_assembly_file, emit_object_file};
use rune::module_loader::load_program_from_path;
use rune::optimize::optimize_program;
use rune::semantic::check_program;

fn temp_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rune-llvm-backend-test-{stamp}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

#[test]
fn emits_linux_object_file_via_inkwell_backend() {
    let dir = temp_dir();
    let source_path = dir.join("main.rn");
    let output_path = dir.join("main.o");
    fs::write(
        &source_path,
        "def main() -> i32:\n    println(42)\n    return 0\n",
    )
    .expect("failed to write source");

    let mut program = load_program_from_path(&source_path).expect("program should load");
    check_program(&program).expect("program should type check");
    optimize_program(&mut program);
    emit_object_file(&program, "x86_64-unknown-linux-gnu", &output_path)
        .expect("inkwell object emission should succeed");

    let bytes = fs::read(&output_path).expect("object file should exist");
    assert!(bytes.starts_with(&[0x7F, b'E', b'L', b'F']));
}

#[test]
fn emits_linux_assembly_file_via_llvm_backend() {
    let dir = temp_dir();
    let source_path = dir.join("main.rn");
    let output_path = dir.join("main.s");
    fs::write(
        &source_path,
        "def main() -> i32:\n    println(42)\n    return 0\n",
    )
    .expect("failed to write source");

    let mut program = load_program_from_path(&source_path).expect("program should load");
    check_program(&program).expect("program should type check");
    optimize_program(&mut program);
    emit_assembly_file(&program, "x86_64-unknown-linux-gnu", &output_path)
        .expect("llvm assembly emission should succeed");

    let asm = fs::read_to_string(&output_path).expect("assembly file should exist");
    assert!(asm.contains("main:"));
    assert!(asm.contains("callq"));
}

#[test]
fn emits_linux_assembly_file_for_script_style_source() {
    let dir = temp_dir();
    let source_path = dir.join("script_main.rn");
    let output_path = dir.join("script_main.s");
    fs::write(&source_path, "println(\"Hello World boi\")\n").expect("failed to write source");

    let mut program = load_program_from_path(&source_path).expect("script should load");
    check_program(&program).expect("script should type check");
    optimize_program(&mut program);
    emit_assembly_file(&program, "x86_64-unknown-linux-gnu", &output_path)
        .expect("llvm assembly emission should succeed for script source");

    let asm = fs::read_to_string(&output_path).expect("assembly file should exist");
    assert!(asm.contains("main:"));
}
