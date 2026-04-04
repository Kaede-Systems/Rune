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
        "import math\n\ndef main() -> i32:\n    return math.add(20, 22)\n",
    )
    .unwrap();

    let program = load_program_from_path(&dir.join("main.rn")).unwrap();
    assert_eq!(program.items.len(), 2);
    assert!(matches!(&program.items[0], Item::Function(function) if function.name.ends_with("__add")));
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
    assert!(error.to_string().contains("E2003"));
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
    assert!(matches!(&program.items[0], Item::Function(function) if function.name.ends_with("__add")));
}

#[test]
fn loads_builtin_env_time_sys_system_io_terminal_fs_json_audio_network_serial_gpio_pwm_and_adc_modules_from_registry() {
    let dir = temp_dir();
    fs::write(
        dir.join("main.rn"),
        "from env import get_or_empty\nfrom time import monotonic_ms\nfrom sys import platform\nfrom system import cpu_count\nfrom io import writeln\nfrom terminal import home\nfrom fs import exists\nfrom json import parse, kind\nfrom audio import beep\nfrom network import connect\nfrom serial import serial_port\nfrom gpio import gpio_pin\nfrom pwm import pwm_pin\nfrom adc import adc_pin\n\ndef main() -> i32:\n    let serial = serial_port(\"COM5\", 115200)\n    let pin = gpio_pin(13)\n    let pwm = pwm_pin(9)\n    let adc = adc_pin(0)\n    home()\n    writeln(get_or_empty(\"RUNE_TEST\"))\n    println(monotonic_ms())\n    println(platform())\n    println(cpu_count())\n    println(exists(\"main.rn\"))\n    println(kind(parse(\"1\")))\n    println(str(beep()))\n    println(connect(\"127.0.0.1\", 65535))\n    println(str(serial))\n    println(str(pin))\n    println(str(pwm))\n    println(str(adc))\n    return 0\n",
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
    let audio_path = PathBuf::from("<builtin>/audio");
    let network_path = PathBuf::from("<builtin>/network");
    let serial_path = PathBuf::from("<builtin>/serial");
    let gpio_path = PathBuf::from("<builtin>/gpio");
    let pwm_path = PathBuf::from("<builtin>/pwm");
    let adc_path = PathBuf::from("<builtin>/adc");
    assert!(bundle.sources.contains_key(&env_path));
    assert!(bundle.sources.contains_key(&time_path));
    assert!(bundle.sources.contains_key(&sys_path));
    assert!(bundle.sources.contains_key(&system_path));
    assert!(bundle.sources.contains_key(&io_path));
    assert!(bundle.sources.contains_key(&terminal_path));
    assert!(bundle.sources.contains_key(&fs_path));
    assert!(bundle.sources.contains_key(&json_path));
    assert!(bundle.sources.contains_key(&audio_path));
    assert!(bundle.sources.contains_key(&network_path));
    assert!(bundle.sources.contains_key(&serial_path));
    assert!(bundle.sources.contains_key(&gpio_path));
    assert!(bundle.sources.contains_key(&pwm_path));
    assert!(bundle.sources.contains_key(&adc_path));
    assert!(bundle.function_origins.values().any(|path| path == &env_path));
    assert!(bundle.function_origins.values().any(|path| path == &time_path));
    assert!(bundle.function_origins.values().any(|path| path == &sys_path));
    assert!(bundle.function_origins.values().any(|path| path == &system_path));
    assert!(bundle.function_origins.values().any(|path| path == &io_path));
    assert!(bundle.function_origins.values().any(|path| path == &terminal_path));
    assert!(bundle.function_origins.values().any(|path| path == &fs_path));
    assert!(bundle.function_origins.values().any(|path| path == &json_path));
    assert!(bundle.function_origins.values().any(|path| path == &audio_path));
    assert!(bundle.function_origins.values().any(|path| path == &network_path));
    assert!(bundle.function_origins.values().any(|path| path == &serial_path));
    assert!(bundle.function_origins.values().any(|path| path == &gpio_path));
    assert!(bundle.function_origins.values().any(|path| path == &pwm_path));
    assert!(bundle.function_origins.values().any(|path| path == &adc_path));
}

#[test]
fn loads_namespaced_modules_with_overlapping_exports() {
    let dir = temp_dir();
    fs::write(
        dir.join("left.rn"),
        "def pin() -> i32:\n    return 10\n",
    )
    .unwrap();
    fs::write(
        dir.join("right.rn"),
        "def pin() -> i32:\n    return 20\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.rn"),
        "import left\nimport right\n\ndef main() -> i32:\n    println(left.pin())\n    println(right.pin())\n    return 0\n",
    )
    .unwrap();

    let bundle = load_program_bundle_from_path(&dir.join("main.rn")).unwrap();
    assert!(
        bundle
            .function_origins
            .keys()
            .any(|name| name.contains("__mod_") && name.ends_with("__pin"))
    );
}

#[test]
fn rejects_import_cycles_with_trace() {
    let dir = temp_dir();
    fs::write(
        dir.join("left.rn"),
        "import right\n\ndef left_value() -> i32:\n    return right.right_value()\n",
    )
    .unwrap();
    fs::write(
        dir.join("right.rn"),
        "import left\n\ndef right_value() -> i32:\n    return left.left_value()\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.rn"),
        "import left\n\ndef main() -> i32:\n    return left.left_value()\n",
    )
    .unwrap();

    let error = load_program_from_path(&dir.join("main.rn")).expect_err("cycle should fail");
    let rendered = error.render();
    assert!(rendered.contains("E2004"));
    assert!(rendered.contains("import cycle detected"));
    assert!(rendered.contains("imported `left`") || rendered.contains("imported `right`"));
}
