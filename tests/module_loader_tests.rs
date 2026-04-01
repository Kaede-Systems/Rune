use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rune::module_loader::load_program_from_path;
use rune::parser::Item;

fn temp_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rune-module-test-{stamp}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

#[test]
fn loads_local_imports() {
    let dir = temp_dir();
    fs::write(
        dir.join("math.rn"),
        "def add(a: i32, b: i32) -> i32:\n    return a + b\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.rn"),
        "import math\n\ndef main() -> i32:\n    return add(20, 22)\n",
    )
    .unwrap();

    let program = load_program_from_path(&dir.join("main.rn")).unwrap();
    assert_eq!(program.items.len(), 2);
    assert!(matches!(&program.items[0], Item::Function(function) if function.name == "add"));
    assert!(matches!(&program.items[1], Item::Function(function) if function.name == "main"));
}

#[test]
fn rejects_missing_imported_name() {
    let dir = temp_dir();
    fs::write(
        dir.join("math.rn"),
        "def add(a: i32, b: i32) -> i32:\n    return a + b\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.rn"),
        "from math import sub\n\ndef main() -> i32:\n    return 0\n",
    )
    .unwrap();

    let error = load_program_from_path(&dir.join("main.rn")).expect_err("load should fail");
    assert!(error.to_string().contains("does not export `sub`"));
}

#[test]
fn loads_relative_imports() {
    let dir = temp_dir();
    fs::create_dir_all(dir.join("pkg")).unwrap();
    fs::write(
        dir.join("pkg").join("math.rn"),
        "def add(a: i32, b: i32) -> i32:\n    return a + b\n",
    )
    .unwrap();
    fs::write(
        dir.join("pkg").join("main.rn"),
        "from .math import add\n\ndef main() -> i32:\n    return add(20, 22)\n",
    )
    .unwrap();

    let program = load_program_from_path(&dir.join("pkg").join("main.rn")).unwrap();
    assert_eq!(program.items.len(), 2);
    assert!(matches!(&program.items[0], Item::Function(function) if function.name == "add"));
}
