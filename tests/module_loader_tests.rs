use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rune::module_loader::{load_program_bundle_from_path, load_program_from_path};
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

#[test]
fn loads_builtin_env_time_sys_system_io_terminal_fs_json_network_serial_and_gpio_modules_from_registry() {
    let dir = temp_dir();
    fs::write(
        dir.join("main.rn"),
        "from env import get_or_empty\nfrom time import monotonic_ms\nfrom sys import platform\nfrom system import cpu_count\nfrom io import writeln\nfrom terminal import home\nfrom fs import exists\nfrom json import parse, kind\nfrom network import connect\nfrom serial import serial_port\nfrom gpio import gpio_pin\n\ndef main() -> i32:\n    let serial = serial_port(\"COM5\", 115200)\n    let pin = gpio_pin(13)\n    home()\n    writeln(get_or_empty(\"RUNE_TEST\"))\n    println(monotonic_ms())\n    println(platform())\n    println(cpu_count())\n    println(exists(\"main.rn\"))\n    println(kind(parse(\"1\")))\n    println(connect(\"127.0.0.1\", 65535))\n    println(str(serial))\n    println(str(pin))\n    return 0\n",
    )
    .unwrap();

    let bundle = load_program_bundle_from_path(&dir.join("main.rn")).unwrap();
    let env_path = PathBuf::from("<builtin>/env");
    let time_path = PathBuf::from("<builtin>/time");
    let sys_path = PathBuf::from("<builtin>/sys");
    let system_path = PathBuf::from("<builtin>/system");
    let io_path = PathBuf::from("<builtin>/io");
    let terminal_path = PathBuf::from("<builtin>/terminal");
    let fs_path = PathBuf::from("<builtin>/fs");
    let json_path = PathBuf::from("<builtin>/json");
    let network_path = PathBuf::from("<builtin>/network");
    let serial_path = PathBuf::from("<builtin>/serial");
    let gpio_path = PathBuf::from("<builtin>/gpio");
    assert!(bundle.sources.contains_key(&env_path));
    assert!(bundle.sources.contains_key(&time_path));
    assert!(bundle.sources.contains_key(&sys_path));
    assert!(bundle.sources.contains_key(&system_path));
    assert!(bundle.sources.contains_key(&io_path));
    assert!(bundle.sources.contains_key(&terminal_path));
    assert!(bundle.sources.contains_key(&fs_path));
    assert!(bundle.sources.contains_key(&json_path));
    assert!(bundle.sources.contains_key(&network_path));
    assert!(bundle.sources.contains_key(&serial_path));
    assert!(bundle.sources.contains_key(&gpio_path));
    assert_eq!(bundle.function_origins.get("get_or_empty"), Some(&env_path));
    assert_eq!(bundle.function_origins.get("monotonic_ms"), Some(&time_path));
    assert_eq!(bundle.function_origins.get("cpu_count"), Some(&system_path));
    assert_eq!(bundle.function_origins.get("writeln"), Some(&io_path));
    assert_eq!(bundle.function_origins.get("home"), Some(&terminal_path));
    assert_eq!(bundle.function_origins.get("exists"), Some(&fs_path));
    assert_eq!(bundle.function_origins.get("kind"), Some(&json_path));
    assert!(
        bundle.function_origins.get("platform") == Some(&sys_path)
            || bundle.function_origins.get("platform") == Some(&system_path)
    );
    assert_eq!(bundle.function_origins.get("connect"), Some(&network_path));
    assert_eq!(bundle.function_origins.get("tcp_client"), Some(&network_path));
    assert_eq!(bundle.function_origins.get("serial_port"), Some(&serial_path));
    assert_eq!(bundle.function_origins.get("gpio_pin"), Some(&gpio_path));
}
