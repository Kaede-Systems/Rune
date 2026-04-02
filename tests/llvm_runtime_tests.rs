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
fn llvm_backend_builds_and_runs_bool_output_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_bool_output_demo.rn");
    let exe_path = dir.join("llvm_bool_output_demo.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    println(true)\n    eprintln(false)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm bool output program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    let stderr = String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n");
    assert_eq!(stdout, "true\n");
    assert_eq!(stderr, "false\n");
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

#[test]
fn llvm_backend_builds_and_runs_json_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_json_demo.rn");
    let exe_path = dir.join("llvm_json_demo.exe");

    fs::write(
        &source_path,
        r#"from json import parse, get, index, to_string, to_i64, to_bool, kind

def main() -> i32:
    let doc: Json = parse("{\"name\":\"Rune\",\"nums\":[40,41,42],\"ok\":true}")
    let left: Json = parse("{\"a\":1,\"b\":[2,3]}")
    let right: Json = parse("{\"b\":[2,3],\"a\":1}")
    println(kind(doc))
    println(to_string(get(doc, "name")))
    println(to_i64(index(get(doc, "nums"), 2)))
    println(str(to_bool(get(doc, "ok"))))
    println(str(left == right))
    println(str(left != parse("{\"a\":1,\"b\":[2,4]}")))
    return 0
"#,
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm json program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built json executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "object\nRune\n42\ntrue\ntrue\ntrue\n");
}

#[test]
fn llvm_backend_builds_and_runs_class_return_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_class_return_demo.rn");
    let exe_path = dir.join("llvm_class_return_demo.exe");

    fs::write(
        &source_path,
        "class Point:\n    x: i32\n    y: i32\n\n\
         def make_point() -> Point:\n    return Point(x=20, y=22)\n\n\
         def main() -> i32:\n    let point: Point = make_point()\n    println(point.x)\n    println(point.y)\n    println(point.x + point.y)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm class return program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built class return executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "20\n22\n42\n");
}

#[test]
fn llvm_backend_builds_and_runs_fs_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_fs_demo.rn");
    let exe_path = dir.join("llvm_fs_demo.exe");
    let file_path = dir.join("llvm_note.txt");
    let rune_file_path = file_path.display().to_string().replace('\\', "/");

    fs::write(
        &source_path,
        format!(
            r#"from fs import write_string, read_string, copy, rename, remove

def main() -> i32:
    println(write_string("{0}", "llvm fs"))
    println(read_string("{0}"))
    println(copy("{0}", "{0}.copy"))
    println(rename("{0}.copy", "{0}.moved"))
    println(remove("{0}.moved"))
    return 0
"#,
            rune_file_path
        ),
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm fs program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built fs executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "true\nllvm fs\ntrue\ntrue\ntrue\n");
}

#[test]
fn llvm_backend_builds_and_runs_class_method_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_class_method_demo.rn");
    let exe_path = dir.join("llvm_class_method_demo.exe");

    fs::write(
        &source_path,
        "class Point:\n    x: i32\n    y: i32\n    def sum(self) -> i32:\n        return self.x + self.y\n\n\
         def main() -> i32:\n    let point: Point = Point(x=20, y=22)\n    println(point.sum())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm class method program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built class method executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "42\n");
}

#[test]
fn llvm_backend_builds_and_runs_object_method_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_class_object_method_demo.rn");
    let exe_path = dir.join("llvm_class_object_method_demo.exe");

    fs::write(
        &source_path,
        "class Counter:\n    value: i32\n    def bump(self) -> Counter:\n        return Counter(value=self.value + 1)\n    def add(self, other: Counter) -> i32:\n        return self.value + other.value\n\n\
         def main() -> i32:\n    let left: Counter = Counter(value=4)\n    let right: Counter = Counter(value=8)\n    let next: Counter = left.bump()\n    println(next.value)\n    println(left.add(right))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm object method program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built object method executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "5\n12\n");
}

#[test]
fn llvm_backend_builds_and_runs_network_send_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_network_send_demo.rn");
    let exe_path = dir.join("llvm_network_send_demo.exe");

    fs::write(
        &source_path,
        r#"from network import tcp_send, udp_send

def main() -> i32:
    println(tcp_send("127.0.0.1", 65535, "hello"))
    println(udp_send("127.0.0.1", 9, "ping"))
    return 0
"#,
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm network send program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built network send executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "false\ntrue\n");
}
