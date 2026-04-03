use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, UdpSocket};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
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

fn spawn_tcp_write_server_on_port(port: u16, payload: &'static [u8]) -> thread::JoinHandle<()> {
    let listener =
        TcpListener::bind(("127.0.0.1", port)).expect("failed to bind TCP listener on test port");
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("failed to accept TCP client");
        stream
            .write_all(payload)
            .expect("failed to write TCP payload");
    })
}

fn spawn_tcp_request_server_on_port(
    port: u16,
    response: &'static [u8],
) -> thread::JoinHandle<()> {
    let listener =
        TcpListener::bind(("127.0.0.1", port)).expect("failed to bind TCP listener on test port");
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("failed to accept TCP client");
        let mut request = [0u8; 256];
        let read = stream.read(&mut request).expect("failed to read request");
        let request_text = String::from_utf8_lossy(&request[..read]).to_string();
        assert_eq!(request_text, "ping\n");
        stream
            .write_all(response)
            .expect("failed to write TCP response");
    })
}

fn spawn_udp_send_server_on_port(port: u16, payload: &'static [u8]) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let sender = UdpSocket::bind("127.0.0.1:0").expect("failed to bind UDP sender");
        for _ in 0..20 {
            thread::sleep(std::time::Duration::from_millis(100));
            sender
                .send_to(payload, ("127.0.0.1", port))
                .expect("failed to send UDP payload");
        }
    })
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
fn llvm_backend_builds_and_runs_arduino_random_and_shift_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_arduino_random_shift.rn");
    let exe_path = dir.join("llvm_arduino_random_shift.exe");

    fs::write(
        &source_path,
        "from arduino import bit_order_msb_first, interrupts_disable, interrupts_enable, random_i64, random_range, random_seed, shift_in\n\n\
         def main() -> i32:\n    interrupts_disable()\n    random_seed(123)\n    let first: i64 = random_i64(10)\n    let second: i64 = random_range(5, 9)\n    interrupts_enable()\n    println(first >= 0 and first < 10)\n    println(second >= 5 and second < 9)\n    println(shift_in(8, 7, bit_order_msb_first()))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm arduino random/shift program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "true\ntrue\n0\n");
}

#[test]
fn llvm_backend_builds_and_runs_break_continue_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_break_continue_demo.rn");
    let exe_path = dir.join("llvm_break_continue_demo.exe");

    fs::write(
        &source_path,
        "def main() -> i32:\n    let value = 0\n    while value < 5:\n        value = value + 1\n        if value == 2:\n            continue\n        println(value)\n        if value == 4:\n            break\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm break/continue program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "1\n3\n4\n");
}

#[test]
fn llvm_backend_builds_and_runs_class_method_program_with_keyword_args_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_class_method_keywords_demo.rn");
    let exe_path = dir.join("llvm_class_method_keywords_demo.exe");

    fs::write(
        &source_path,
        "class Mixer:\n    base: i32\n    def combine(self, left: i32, right: i32) -> i32:\n        return self.base + left + right\n\n\
         def main() -> i32:\n    let mixer: Mixer = Mixer(base=10)\n    println(mixer.combine(right=8, left=4))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm class method keyword-arg program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built class method keyword-arg executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "22\n");
}

#[test]
fn llvm_backend_builds_and_runs_inline_constructor_method_program_with_keyword_args_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_class_inline_method_keywords_demo.rn");
    let exe_path = dir.join("llvm_class_inline_method_keywords_demo.exe");

    fs::write(
        &source_path,
        "class Mixer:\n    base: i32\n    def combine(self, left: i32, right: i32) -> i32:\n        return self.base + left + right\n\n\
         def main() -> i32:\n    println(Mixer(base=10).combine(right=8, left=4))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm inline constructor method keyword-arg program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built inline constructor method keyword-arg executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "22\n");
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
fn llvm_backend_builds_and_runs_string_returning_method_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_class_string_method_demo.rn");
    let exe_path = dir.join("llvm_class_string_method_demo.exe");

    fs::write(
        &source_path,
        "class Greeter:\n    name: String\n    def greet(self) -> String:\n        return \"hi \" + self.name\n\n\
         def main() -> i32:\n    let greeter = Greeter(name=\"Rune\")\n    println(greeter.greet())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm string-returning method program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built string-returning method executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "hi Rune\n");
}

#[test]
fn llvm_backend_builds_and_runs_str_magic_method_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_class_str_magic_demo.rn");
    let exe_path = dir.join("llvm_class_str_magic_demo.exe");

    fs::write(
        &source_path,
        "class Counter:\n    value: i32\n    def __str__(self) -> String:\n        return \"Counter(\" + str(self.value) + \")\"\n\n\
         def main() -> i32:\n    println(str(Counter(value=5)))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm str magic method program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built str magic method executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "Counter(5)\n");
}

#[test]
fn llvm_backend_builds_and_runs_default_object_string_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_class_default_str_demo.rn");
    let exe_path = dir.join("llvm_class_default_str_demo.exe");

    fs::write(
        &source_path,
        "class Counter:\n    value: i32\n\n\
         def main() -> i32:\n    println(str(Counter(value=5)))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm default object string program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built default object string executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "Counter(value=5)\n");
}

#[test]
fn llvm_backend_builds_and_runs_direct_print_object_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_class_direct_print_demo.rn");
    let exe_path = dir.join("llvm_class_direct_print_demo.exe");

    fs::write(
        &source_path,
        "class Counter:\n    value: i32\n\n\
         def main() -> i32:\n    println(Counter(value=5))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm direct object print program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built direct object print executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "Counter(value=5)\n");
}

#[test]
fn llvm_backend_builds_and_runs_cli_arg_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_cli_args_demo.rn");
    let exe_path = dir.join("llvm_cli_args_demo.exe");

    fs::write(
        &source_path,
        "from env import arg, arg_count\n\n\
         def main() -> i32:\n    println(arg_count())\n    println(arg(0))\n    println(arg(1))\n    println(arg(2))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm cli arg program should build");

    let output = Command::new(&exe_path)
        .arg("--port")
        .arg("COM5")
        .output()
        .expect("failed to run llvm-built cli arg executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "2\n--port\nCOM5\n\n");
}

#[test]
fn llvm_backend_builds_and_runs_env_string_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_env_string_demo.rn");
    let exe_path = dir.join("llvm_env_string_demo.exe");

    fs::write(
        &source_path,
        "from env import get, get_or_empty\n\n\
         def main() -> i32:\n    println(get(\"RUNE_LLVM_HOST\", \"fallback-host\"))\n    println(get_or_empty(\"RUNE_LLVM_EMPTY\"))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm env string program should build");

    let output = Command::new(&exe_path)
        .env("RUNE_LLVM_HOST", "llvm-host")
        .output()
        .expect("failed to run llvm-built env string executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "llvm-host\n\n");
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

#[test]
fn llvm_backend_builds_and_runs_network_class_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_network_class_demo.rn");
    let exe_path = dir.join("llvm_network_class_demo.exe");

    fs::write(
        &source_path,
        r#"from network import tcp_client, udp_endpoint

def main() -> i32:
    let tcp = tcp_client("127.0.0.1", 65535)
    let udp = udp_endpoint("127.0.0.1", 9)
    println(tcp.probe())
    println(udp.send_line("ping"))
    return 0
"#,
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm network class program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built network class executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "false\ntrue\n");
}

#[test]
fn llvm_backend_builds_and_runs_network_alias_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_network_alias_demo.rn");
    let exe_path = dir.join("llvm_network_alias_demo.exe");

    fs::write(
        &source_path,
        r#"from network import connect, connect_timeout, probe, probe_timeout, listen, bind, send, send_line

def main() -> i32:
    println(connect("127.0.0.1", 65535))
    println(connect_timeout("127.0.0.1", 65535, 1))
    println(probe("127.0.0.1", 65535))
    println(probe_timeout("127.0.0.1", 65535, 1))
    println(listen("127.0.0.1", 0))
    println(bind("127.0.0.1", 0))
    println(send("127.0.0.1", 65535, "hello"))
    println(send_line("127.0.0.1", 65535, "world"))
    return 0
"#,
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm network alias program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built network alias executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "false\nfalse\nfalse\nfalse\ntrue\ntrue\nfalse\nfalse\n");
}

#[test]
fn llvm_backend_builds_and_runs_network_send_text_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_network_send_text_demo.rn");
    let exe_path = dir.join("llvm_network_send_text_demo.exe");

    fs::write(
        &source_path,
        r#"from network import tcp_client, udp_endpoint

def main() -> i32:
    let tcp = tcp_client("127.0.0.1", 65535)
    let udp = udp_endpoint("127.0.0.1", 9)
    println(tcp.send_text("hello"))
    println(tcp.send_line_text("world"))
    println(udp.send_text("ping"))
    println(udp.send_line_text("pong"))
    return 0
"#,
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm network send_text program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built network send_text executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "false\nfalse\ntrue\ntrue\n");
}

#[test]
fn llvm_backend_builds_and_runs_network_receive_and_request_program_on_windows() {
    let dir = temp_dir();
    let source_path = dir.join("llvm_network_recv_request_demo.rn");
    let exe_path = dir.join("llvm_network_recv_request_demo.exe");
    let recv_probe = TcpListener::bind("127.0.0.1:0").expect("failed to reserve TCP recv port");
    let recv_port = recv_probe.local_addr().expect("recv probe addr").port() as i32;
    drop(recv_probe);
    let request_probe =
        TcpListener::bind("127.0.0.1:0").expect("failed to reserve TCP request port");
    let request_port = request_probe
        .local_addr()
        .expect("request probe addr")
        .port() as i32;
    drop(request_probe);
    let udp_probe = UdpSocket::bind("127.0.0.1:0").expect("failed to reserve UDP port");
    let udp_port = udp_probe.local_addr().expect("udp probe addr").port() as i32;
    drop(udp_probe);

    fs::write(
        &source_path,
        format!(
            r#"from network import recv, recv_timeout, request_line, recv_udp, tcp_client, udp_endpoint

def main() -> i32:
    let tcp = tcp_client("127.0.0.1", {0})
    let udp = udp_endpoint("127.0.0.1", {2})
    println(recv("127.0.0.1", {0}, 64))
    println(recv_timeout("127.0.0.1", {0}, 64, 500))
    println(request_line("127.0.0.1", {1}, "ping", 64, 500))
    println(tcp.recv(64))
    println(udp.recv(64, 500))
    return 0
"#,
            recv_port, request_port, udp_port
        ),
    )
    .expect("failed to write source");

    build_executable_llvm(&source_path, &exe_path, Some("x86_64-pc-windows-gnu"))
        .expect("llvm network recv/request program should build");

    let _recv_server = spawn_tcp_write_server_on_port(recv_port as u16, b"hello recv");
    let _request_server = spawn_tcp_request_server_on_port(request_port as u16, b"pong");
    let _udp_server = spawn_udp_send_server_on_port(udp_port as u16, b"hello udp");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run llvm-built network recv/request executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "hello recv\n\npong\n\nhello udp\n");
}
