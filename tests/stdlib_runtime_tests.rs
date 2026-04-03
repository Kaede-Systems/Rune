use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, UdpSocket};
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::thread;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rune::build::build_executable;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "rune-stdlib-runtime-test-{}-{}-{}",
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
fn builds_and_runs_stdlib_env_fs_system_time_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_env_fs_system_time.rn");
    let exe_path = dir.join("stdlib_env_fs_system_time.exe");
    let file_path = dir.join("note.txt");
    let moved_path = dir.join("note.txt.moved");
    let rune_file_path = file_path.display().to_string().replace('\\', "/");

    let source = format!(
        r#"from env import get, get_i32_or_zero, get_bool_or_false
from fs import exists, read_string, write_text, copy, rename, remove
from system import pid, cpu_count
from time import monotonic_ms, monotonic_us, sleep_until, sleep_until_us

def main() -> i32:
    if exists("{0}"):
        println("exists-before")
    else:
        println("missing-before")
    if write_text("{0}", "hello stdlib"):
        println("write-ok")
    else:
        println("write-fail")
    println(read_string("{0}"))
    if copy("{0}", "{0}.copy"):
        println("copy-ok")
    else:
        println("copy-fail")
    if rename("{0}.copy", "{0}.moved"):
        println("rename-ok")
    else:
        println("rename-fail")
    if remove("{0}.moved"):
        println("remove-ok")
    else:
        println("remove-fail")
    println(get("RUNE_STDLIB_HOST", "fallback-host"))
    println(get_i32_or_zero("RUNE_STDLIB_INT"))
    if get_bool_or_false("RUNE_STDLIB_BOOL"):
        println("bool-yes")
    else:
        println("bool-no")
    let start: i64 = monotonic_ms()
    sleep_until(start)
    let start_us: i64 = monotonic_us()
    sleep_until_us(start_us)
    if monotonic_us() >= start_us:
        println("mono-us-ok")
    else:
        println("mono-us-bad")
    if pid() > 0:
        println("pid-ok")
    else:
        println("pid-bad")
    if cpu_count() > 0:
        println("cpu-ok")
    else:
        println("cpu-bad")
    return 0
"#,
        rune_file_path,
    );
    fs::write(&source_path, source).expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("stdlib helper program should build");

    let output = Command::new(&exe_path)
        .env("RUNE_STDLIB_HOST", "rune-host")
        .env("RUNE_STDLIB_INT", "17")
        .env("RUNE_STDLIB_BOOL", "true")
        .output()
        .expect("failed to run stdlib helper program");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(
        stdout,
        "missing-before\nwrite-ok\nhello stdlib\ncopy-ok\nrename-ok\nremove-ok\nrune-host\n17\nbool-yes\nmono-us-ok\npid-ok\ncpu-ok\n"
    );
    let written = fs::read_to_string(&file_path).expect("stdlib write_text should create file");
    assert_eq!(written, "hello stdlib");
    assert!(!moved_path.exists());
}

#[test]
fn builds_and_runs_stdlib_sys_platform_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_sys_platform.rn");
    let exe_path = dir.join("stdlib_sys_platform.exe");

    fs::write(
        &source_path,
        "from sys import platform, arch, target, board, is_embedded, is_wasm, is_host, is_desktop, is_windows, is_linux, is_macos\n\n\
         def main() -> i32:\n    println(platform())\n    println(arch())\n    println(target())\n    println(board())\n    println(str(is_embedded()))\n    println(str(is_wasm()))\n    println(str(is_host()))\n    println(str(is_desktop()))\n    println(str(is_windows() or is_linux() or is_macos()))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("sys program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run built executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert!(stdout.contains("windows\n") || stdout.contains("linux\n") || stdout.contains("macos\n") || stdout.contains("wasi\n"));
    assert!(stdout.contains("x86_64\n") || stdout.contains("aarch64\n"));
    assert!(stdout.contains("host\n"));
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(lines.len() >= 9, "unexpected sys output: {stdout:?}");
    assert_eq!(lines[4], "false");
    assert_eq!(lines[5], "false");
    assert_eq!(lines[6], "true");
    assert_eq!(lines[7], "true");
    assert_eq!(lines[8], "true");
}

#[test]
fn builds_and_runs_stdlib_terminal_and_audio_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_terminal_audio.rn");
    let exe_path = dir.join("stdlib_terminal_audio.exe");

    fs::write(
        &source_path,
        r#"from terminal import cursor_hide, cursor_show, clear_screen, move_cursor, title
from audio import beep

def main() -> i32:
    cursor_hide()
    cursor_show()
    clear_screen()
    move_cursor(1, 1)
    title("Rune Test")
    println(beep())
    return 0
"#,
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("terminal/audio stdlib program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run terminal/audio stdlib program");

    assert!(output.status.success());
    let stdout = output.stdout;
    assert!(stdout.windows(6).any(|chunk| chunk == b"\x1b[?25l"));
    assert!(stdout.windows(6).any(|chunk| chunk == b"\x1b[?25h"));
    assert!(stdout.windows(7).any(|chunk| chunk == b"\x1b[2J\x1b[H"));
    assert!(stdout.contains(&b'\x07'));
    let text = String::from_utf8_lossy(&stdout).replace("\r\n", "\n");
    assert!(text.ends_with("\x07true\n"), "unexpected terminal/audio output: {text:?}");
}

#[test]
fn builds_and_runs_stdlib_io_and_env_ergonomics_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_io_env_ergonomics.rn");
    let exe_path = dir.join("stdlib_io_env_ergonomics.exe");

    fs::write(
        &source_path,
        r#"from io import prompt, stdout_write, stdout_writeln, stderr_write, stderr_writeln, flush_stdout, flush_stderr
from env import arg_count, arg_or, get_or_empty

def main() -> i32:
    stdout_write("prompt>")
    let answer: String = prompt(" ")
    stdout_writeln(answer)
    stderr_write("err>")
    stderr_writeln("ok")
    flush_stdout()
    flush_stderr()
    println(get_or_empty("RUNE_IO_ENV_LABEL"))
    println(arg_count())
    println(arg_or(1, "missing"))
    println(arg_or(9, "missing"))
    return 0
"#,
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("io/env ergonomics program should build");

    let mut child = Command::new(&exe_path)
        .arg("--port")
        .arg("COM5")
        .env("RUNE_IO_ENV_LABEL", "serial-cli")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run io/env ergonomics program");

    child
        .stdin
        .as_mut()
        .expect("stdin should be piped")
        .write_all(b"hello rune\n")
        .expect("failed to write stdin");

    let output = child
        .wait_with_output()
        .expect("failed to wait for io/env ergonomics program");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    let stderr = String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n");
    assert_eq!(stdout, "prompt> hello rune\nserial-cli\n2\nCOM5\nmissing\n");
    assert_eq!(stderr, "err>ok\n");
}

#[test]
fn builds_and_runs_stdlib_network_alias_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_network_alias.rn");
    let exe_path = dir.join("stdlib_network_alias.exe");

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

    build_executable(&source_path, &exe_path, None).expect("network stdlib alias program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run network stdlib alias program");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "false\nfalse\nfalse\nfalse\ntrue\ntrue\nfalse\nfalse\n");
}

#[test]
fn builds_and_runs_stdlib_network_send_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_network_send.rn");
    let exe_path = dir.join("stdlib_network_send.exe");

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

    build_executable(&source_path, &exe_path, None)
        .expect("network stdlib send program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run network stdlib send program");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "false\ntrue\n");
}

#[test]
fn builds_and_runs_stdlib_network_class_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_network_class.rn");
    let exe_path = dir.join("stdlib_network_class.exe");

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

    build_executable(&source_path, &exe_path, None)
        .expect("network stdlib class program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run network stdlib class program");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "false\ntrue\n");
}

#[test]
fn builds_and_runs_stdlib_network_send_text_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_network_send_text.rn");
    let exe_path = dir.join("stdlib_network_send_text.exe");

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

    build_executable(&source_path, &exe_path, None)
        .expect("network stdlib send_text program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run network stdlib send_text program");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "false\nfalse\ntrue\ntrue\n");
}

#[test]
fn builds_and_runs_stdlib_network_receive_and_request_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_network_recv_request.rn");
    let exe_path = dir.join("stdlib_network_recv_request.exe");
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

    build_executable(&source_path, &exe_path, None)
        .expect("network recv/request program should build");

    let _recv_server = spawn_tcp_write_server_on_port(recv_port as u16, b"hello recv");
    let _request_server = spawn_tcp_request_server_on_port(request_port as u16, b"pong");
    let _udp_server = spawn_udp_send_server_on_port(udp_port as u16, b"hello udp");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run network recv/request program");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "hello recv\n\npong\n\nhello udp\n");
}


#[test]
fn builds_and_runs_stdlib_io_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_io.rn");
    let exe_path = dir.join("stdlib_io.exe");

    fs::write(
        &source_path,
        r#"from io import write, writeln, error, errorln, flush_out, flush_err, read_line

def main() -> i32:
    write("out=")
    writeln(read_line())
    error("err=")
    errorln("warn")
    flush_out()
    flush_err()
    return 0
"#,
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("io stdlib program should build");

    let mut child = Command::new(&exe_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run io stdlib program");

    child
        .stdin
        .as_mut()
        .expect("stdin should be piped")
        .write_all(b"hello io\n")
        .expect("failed to write stdin");

    let output = child
        .wait_with_output()
        .expect("failed to wait for io stdlib program");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    let stderr = String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n");
    assert_eq!(stdout, "out=hello io\n");
    assert_eq!(stderr, "err=warn\n");
}

#[test]
fn builds_and_runs_stdlib_json_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_json.rn");
    let exe_path = dir.join("stdlib_json.exe");

    fs::write(
        &source_path,
        r#"from json import parse, stringify, kind, is_null, len, get, index, to_string, to_i64, to_bool

def main() -> i32:
    let doc: Json = parse("{\"name\":\"Rune\",\"nums\":[1,2,3],\"ok\":true,\"none\":null}")
    let left: Json = parse("{\"a\":1,\"b\":[2,3]}")
    let right: Json = parse("{\"b\":[2,3],\"a\":1}")
    println(kind(doc))
    println(stringify(doc))
    println(to_string(get(doc, "name")))
    println(to_i64(index(get(doc, "nums"), 2)))
    println(str(to_bool(get(doc, "ok"))))
    println(str(is_null(get(doc, "none"))))
    println(len(get(doc, "nums")))
    println(str(left == right))
    println(str(left != parse("{\"a\":1,\"b\":[2,4]}")))
    return 0
"#,
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("json stdlib program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run json stdlib program");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(
        stdout,
        "object\n{\"name\":\"Rune\",\"nums\":[1,2,3],\"ok\":true,\"none\":null}\nRune\n3\ntrue\ntrue\n3\ntrue\ntrue\n"
    );
}

#[test]
fn builds_and_runs_stdlib_fs_json_ergonomics_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_fs_json_ergonomics.rn");
    let exe_path = dir.join("stdlib_fs_json_ergonomics.exe");
    let file_path = dir.join("payload.json");
    let moved_path = dir.join("payload.json.moved");
    let rune_file_path = file_path.display().to_string().replace('\\', "/");
    let rune_moved_path = moved_path.display().to_string().replace('\\', "/");

    fs::write(
        &source_path,
        format!(
            r#"from fs import exists, read, write, delete, move, mkdir_p
from json import parse, get, index, value_kind, as_string, as_i64, as_bool, stringify

def main() -> i32:
    let base: String = "{0}"
    let moved: String = "{1}"
    if mkdir_p("{2}"):
        println("mkdir-ok")
    else:
        println("mkdir-fail")
    if write(base, "{{\"name\":\"Rune\",\"nums\":[10,20,30],\"ok\":true}}"):
        println("write-ok")
    else:
        println("write-fail")
    println(str(exists(base)))
    println(read(base))
    let doc: Json = parse(read(base))
    println(value_kind(get(doc, "name")))
    println(as_string(get(doc, "name")))
    println(as_i64(index(get(doc, "nums"), 1)))
    println(str(as_bool(get(doc, "ok"))))
    println(stringify(doc))
    if move(base, moved):
        println("move-ok")
    else:
        println("move-fail")
    if delete(moved):
        println("delete-ok")
    else:
        println("delete-fail")
    return 0
"#,
            rune_file_path,
            rune_moved_path,
            dir.join("out").display().to_string().replace('\\', "/"),
        ),
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("fs/json ergonomics program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run fs/json ergonomics program");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(
        stdout,
        "mkdir-ok\nwrite-ok\ntrue\n{\"name\":\"Rune\",\"nums\":[10,20,30],\"ok\":true}\nstring\nRune\n20\ntrue\n{\"name\":\"Rune\",\"nums\":[10,20,30],\"ok\":true}\nmove-ok\ndelete-ok\n"
    );
    assert!(!file_path.exists());
    assert!(!moved_path.exists());
}
