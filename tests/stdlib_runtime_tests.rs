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

fn spawn_tcp_client_send_on_port(port: u16, payload: &'static [u8]) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for _ in 0..40 {
            match std::net::TcpStream::connect(("127.0.0.1", port)) {
                Ok(mut stream) => {
                    stream
                        .write_all(payload)
                        .expect("failed to write TCP client payload");
                    return;
                }
                Err(_) => thread::sleep(std::time::Duration::from_millis(25)),
            }
        }
        panic!("failed to connect TCP client to test server");
    })
}

fn spawn_tcp_client_request_on_port(
    port: u16,
    payload: &'static [u8],
    expected_response: &'static str,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for _ in 0..40 {
            match std::net::TcpStream::connect(("127.0.0.1", port)) {
                Ok(mut stream) => {
                    stream
                        .write_all(payload)
                        .expect("failed to write TCP request payload");
                    let mut response = [0u8; 128];
                    let read = stream.read(&mut response).expect("failed to read reply");
                    let text = String::from_utf8_lossy(&response[..read]).to_string();
                    assert_eq!(text, expected_response);
                    return;
                }
                Err(_) => thread::sleep(std::time::Duration::from_millis(25)),
            }
        }
        panic!("failed to connect TCP client to reply server");
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
fn builds_and_runs_stdlib_network_server_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_network_server.rn");
    let exe_path = dir.join("stdlib_network_server.exe");
    let accept_probe = TcpListener::bind("127.0.0.1:0").expect("failed to reserve accept port");
    let accept_port = accept_probe.local_addr().expect("accept probe addr").port() as i32;
    drop(accept_probe);
    let reply_probe = TcpListener::bind("127.0.0.1:0").expect("failed to reserve reply port");
    let reply_port = reply_probe.local_addr().expect("reply probe addr").port() as i32;
    drop(reply_probe);
    let method_probe = TcpListener::bind("127.0.0.1:0").expect("failed to reserve method port");
    let method_port = method_probe.local_addr().expect("method probe addr").port() as i32;
    drop(method_probe);

    fs::write(
        &source_path,
        format!(
            r#"from network import accept_once, reply_once_line, tcp_server

def main() -> i32:
    let server = tcp_server("127.0.0.1", {2})
    println(accept_once("127.0.0.1", {0}, 64, 1000))
    println(reply_once_line("127.0.0.1", {1}, "pong", 64, 1000))
    println(server.reply_once_line("hi", 64, 1000))
    return 0
"#,
            accept_port, reply_port, method_port
        ),
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("network server program should build");

    let child = Command::new(&exe_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run network server program");

    let client_a = spawn_tcp_client_send_on_port(accept_port as u16, b"hello server");
    let client_b = spawn_tcp_client_request_on_port(reply_port as u16, b"ping\n", "pong\n");
    let client_c = spawn_tcp_client_request_on_port(method_port as u16, b"yo\n", "hi\n");

    let output = child
        .wait_with_output()
        .expect("failed to wait for network server program");

    client_a.join().expect("accept client should finish");
    client_b.join().expect("reply client should finish");
    client_c.join().expect("method reply client should finish");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "hello server\nping\n\nyo\n\n");
}

#[test]
fn builds_and_runs_stdlib_network_error_state_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_network_error_state.rn");
    let exe_path = dir.join("stdlib_network_error_state.exe");
    let open_probe = TcpListener::bind("127.0.0.1:0").expect("failed to reserve open TCP port");
    let open_port = open_probe.local_addr().expect("open probe addr").port() as i32;
    let closed_probe =
        TcpListener::bind("127.0.0.1:0").expect("failed to reserve closed TCP port");
    let closed_port = closed_probe
        .local_addr()
        .expect("closed probe addr")
        .port() as i32;
    drop(closed_probe);

    fs::write(
        &source_path,
        format!(
            r#"from network import clear_error, connect_timeout, last_error, last_error_code

def main() -> i32:
    println(connect_timeout("127.0.0.1", {0}, 25))
    println(last_error_code())
    println(last_error() != "")
    clear_error()
    println(last_error_code())
    println(last_error() == "")
    println(connect_timeout("127.0.0.1", {1}, 100))
    println(last_error_code())
    return 0
"#,
            closed_port, open_port
        ),
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("network error-state program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run network error-state program");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "false\n5\ntrue\n0\ntrue\ntrue\n0\n");
}

#[test]
fn builds_and_runs_clock_module_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_clock_demo.rn");
    let exe_path = dir.join("stdlib_clock_demo.exe");

    fs::write(
        &source_path,
        "from clock import has_wall_clock, ticks_ms, ticks_us, elapsed_ms, elapsed_us, sleep_ms, sleep_us, wait_until_ms, wait_until_us\n\n\
def main() -> i32:\n    let start_ms: i64 = ticks_ms()\n    let start_us: i64 = ticks_us()\n    sleep_ms(1)\n    sleep_us(100)\n    wait_until_ms(start_ms)\n    wait_until_us(start_us)\n    println(has_wall_clock())\n    println(elapsed_ms(start_ms) >= 0)\n    println(elapsed_us(start_us) >= 0)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("clock stdlib program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run clock stdlib executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "true\ntrue\ntrue\n");
}

#[test]
fn builds_and_runs_builtin_gpio_digital_program() {
    let dir = temp_dir();
    let source_path = dir.join("builtin_gpio_digital_demo.rn");
    let exe_path = dir.join("builtin_gpio_digital_demo.exe");

    fs::write(
        &source_path,
        "from gpio import gpio_pin\n\n\
def main() -> i32:\n    let pin = gpio_pin(13)\n    pin.output()\n    pin.high()\n    println(pin.read())\n    pin.low()\n    println(pin.read())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("builtin gpio digital program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run builtin gpio digital executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "true\nfalse\n");
}

#[test]
fn builds_and_runs_builtin_gpio_pwm_analog_program() {
    let dir = temp_dir();
    let source_path = dir.join("builtin_gpio_pwm_analog_demo.rn");
    let exe_path = dir.join("builtin_gpio_pwm_analog_demo.exe");

    fs::write(
        &source_path,
        "from gpio import analog_pin, pwm_pin\n\n\
def main() -> i32:\n    let pwm = pwm_pin(9)\n    let sensor = analog_pin(9)\n    pwm.output()\n    pwm.write(64)\n    println(sensor.read())\n    println(sensor.read_percent())\n    println(pwm.max_duty())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("builtin gpio pwm/analog program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run builtin gpio pwm/analog executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "64\n6\n255\n");
}

#[test]
fn builds_and_runs_builtin_gpio_function_surface_program() {
    let dir = temp_dir();
    let source_path = dir.join("builtin_gpio_function_surface_demo.rn");
    let exe_path = dir.join("builtin_gpio_function_surface_demo.exe");

    fs::write(
        &source_path,
        "from gpio import analog_in, analog_in_percent, analog_in_voltage_mv, digital_in, digital_out, pwm_duty_max, pwm_write\n\n\
def main() -> i32:\n    digital_out(7, true)\n    println(digital_in(7))\n    pwm_write(9, 128)\n    println(analog_in(9))\n    println(analog_in_percent(9))\n    println(analog_in_voltage_mv(9, 5000))\n    println(pwm_duty_max())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("builtin gpio function-surface program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run builtin gpio function-surface executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "true\n128\n12\n625\n255\n");
}

#[test]
fn builds_and_runs_builtin_pwm_and_adc_program() {
    let dir = temp_dir();
    let source_path = dir.join("builtin_pwm_adc_demo.rn");
    let exe_path = dir.join("builtin_pwm_adc_demo.exe");

    fs::write(
        &source_path,
        "from pwm import pwm_pin\nfrom adc import adc_pin, max\n\n\
def main() -> i32:\n    let pwm = pwm_pin(9)\n    let sensor = adc_pin(9)\n    pwm.output()\n    pwm.write(64)\n    println(sensor.read())\n    println(sensor.read_percent())\n    println(sensor.read_voltage_mv(5000))\n    println(pwm.max_duty())\n    println(max())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("builtin pwm/adc program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run builtin pwm/adc executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "64\n6\n312\n255\n1023\n");
}

#[test]
fn builds_and_runs_namespaced_builtin_pwm_and_adc_program() {
    let dir = temp_dir();
    let source_path = dir.join("namespaced_builtin_pwm_adc_demo.rn");
    let exe_path = dir.join("namespaced_builtin_pwm_adc_demo.exe");

    fs::write(
        &source_path,
        "import pwm\nimport adc\n\n\
def main() -> i32:\n    let out = pwm.pin(9)\n    let sensor = adc.pin(9)\n    out.output()\n    out.write(64)\n    println(sensor.read())\n    println(sensor.read_percent())\n    println(out.max_duty())\n    println(adc.max())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("namespaced builtin pwm/adc program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run namespaced builtin pwm/adc executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "64\n6\n255\n1023\n");
}

#[test]
fn builds_and_runs_namespaced_local_modules_with_overlapping_exports() {
    let dir = temp_dir();
    let source_path = dir.join("main.rn");
    let exe_path = dir.join("namespaced_local_modules.exe");

    fs::write(dir.join("left.rn"), "def pin() -> i32:\n    return 10\n")
        .expect("failed to write left module");
    fs::write(dir.join("right.rn"), "def pin() -> i32:\n    return 20\n")
        .expect("failed to write right module");
    fs::write(
        &source_path,
        "import left\nimport right\n\ndef main() -> i32:\n    println(left.pin())\n    println(right.pin())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("namespaced local module program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run namespaced local module executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "10\n20\n");
}

#[test]
fn builds_and_runs_serial_timeout_program() {
    let dir = temp_dir();
    let source_path = dir.join("serial_timeout_demo.rn");
    let exe_path = dir.join("serial_timeout_demo.exe");

    fs::write(
        &source_path,
        "from serial import recv_line_timeout, recv_nonempty_timeout\n\n\
def main() -> i32:\n    println(recv_line_timeout(10) == \"\")\n    println(recv_nonempty_timeout(10) == \"\")\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("serial timeout program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run serial timeout executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "true\ntrue\n");
}

#[test]
fn builds_and_runs_serial_flush_program() {
    let dir = temp_dir();
    let source_path = dir.join("serial_flush_demo.rn");
    let exe_path = dir.join("serial_flush_demo.exe");

    fs::write(
        &source_path,
        "from serial import flush\n\ndef main() -> i32:\n    flush()\n    println(\"ok\")\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("serial flush program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run serial flush executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "ok\n");
}

#[test]
fn builds_and_runs_serial_byte_helpers_without_open_program() {
    let dir = temp_dir();
    let source_path = dir.join("serial_byte_helpers_demo.rn");
    let exe_path = dir.join("serial_byte_helpers_demo.exe");

    fs::write(
        &source_path,
        "from serial import peek_byte, write_byte\n\n\
def main() -> i32:\n    println(peek_byte())\n    println(write_byte(65))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("serial byte helper program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run serial byte helper executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "-1\nfalse\n");
}

#[test]
fn builds_and_runs_serial_available_and_read_byte_without_open_program() {
    let dir = temp_dir();
    let source_path = dir.join("serial_available_read_byte_demo.rn");
    let exe_path = dir.join("serial_available_read_byte_demo.exe");

    fs::write(
        &source_path,
        "from serial import available, read_byte\n\n\
def main() -> i32:\n    println(available())\n    println(read_byte())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("serial available/read_byte program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run serial available/read_byte executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "0\n-1\n");
}

#[test]
fn builds_and_runs_serial_read_byte_timeout_without_open_program() {
    let dir = temp_dir();
    let source_path = dir.join("serial_read_byte_timeout_demo.rn");
    let exe_path = dir.join("serial_read_byte_timeout_demo.exe");

    fs::write(
        &source_path,
        "from serial import read_byte_timeout\n\n\
def main() -> i32:\n    println(read_byte_timeout(10))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("serial read_byte_timeout program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run serial read_byte_timeout executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "-1\n");
}

#[test]
fn builds_and_runs_stdlib_network_persistent_server_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_network_persistent_server.rn");
    let exe_path = dir.join("stdlib_network_persistent_server.exe");
    let server_probe = TcpListener::bind("127.0.0.1:0").expect("failed to reserve server port");
    let server_port = server_probe.local_addr().expect("server probe addr").port() as i32;
    drop(server_probe);

    fs::write(
        &source_path,
        format!(
            r#"from network import tcp_server_accept, tcp_server_close, tcp_server_open, tcp_server_reply

def main() -> i32:
    let handle: i32 = tcp_server_open("127.0.0.1", {0})
    println(tcp_server_accept(handle, 64, 1000))
    println(tcp_server_reply(handle, "pong\n", 64, 1000))
    println(tcp_server_close(handle))
    return 0
"#,
            server_port
        ),
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("persistent network server program should build");

    let child = Command::new(&exe_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run persistent network server program");

    let client_a = spawn_tcp_client_send_on_port(server_port as u16, b"alpha");
    client_a.join().expect("accept client should finish");
    let client_b = spawn_tcp_client_request_on_port(server_port as u16, b"beta\n", "pong\n");
    client_b.join().expect("reply client should finish");

    let output = child
        .wait_with_output()
        .expect("failed to wait for persistent network server program");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "alpha\nbeta\n\ntrue\n");
}

#[test]
fn builds_and_runs_stdlib_network_persistent_server_class_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_network_persistent_server_class.rn");
    let exe_path = dir.join("stdlib_network_persistent_server_class.exe");
    let server_probe = TcpListener::bind("127.0.0.1:0").expect("failed to reserve server port");
    let server_port = server_probe.local_addr().expect("server probe addr").port() as i32;
    drop(server_probe);

    fs::write(
        &source_path,
        format!(
            r#"from network import tcp_server

def main() -> i32:
    let server = tcp_server("127.0.0.1", {0})
    let handle: i32 = server.open_handle()
    println(server.accept(handle, 64, 1000))
    println(server.reply_line(handle, "pong", 64, 1000))
    println(server.close_handle(handle))
    return 0
"#,
            server_port
        ),
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("persistent network server class program should build");

    let child = Command::new(&exe_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run persistent network server class program");

    let client_a = spawn_tcp_client_send_on_port(server_port as u16, b"alpha");
    client_a.join().expect("accept client should finish");
    let client_b = spawn_tcp_client_request_on_port(server_port as u16, b"beta\n", "pong\n");
    client_b.join().expect("reply client should finish");

    let output = child
        .wait_with_output()
        .expect("failed to wait for persistent network server class program");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "alpha\nbeta\n\ntrue\n");
}

#[test]
fn builds_and_runs_stdlib_network_persistent_client_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_network_persistent_client.rn");
    let exe_path = dir.join("stdlib_network_persistent_client.exe");
    let server_probe = TcpListener::bind("127.0.0.1:0").expect("failed to reserve server port");
    let server_port = server_probe.local_addr().expect("server probe addr").port() as i32;
    drop(server_probe);

    fs::write(
        &source_path,
        format!(
            r#"from network import tcp_client_close, tcp_client_open, tcp_client_recv, tcp_client_send_line

def main() -> i32:
    let handle: i32 = tcp_client_open("127.0.0.1", {0}, 1000)
    println(tcp_client_send_line(handle, "ping"))
    println(tcp_client_recv(handle, 64, 1000))
    println(tcp_client_close(handle))
    return 0
"#,
            server_port
        ),
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("persistent network client program should build");

    let _server = spawn_tcp_request_server_on_port(server_port as u16, b"pong\n");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run persistent network client program");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "true\npong\n\ntrue\n");
}

#[test]
fn builds_and_runs_stdlib_network_persistent_client_class_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_network_persistent_client_class.rn");
    let exe_path = dir.join("stdlib_network_persistent_client_class.exe");
    let server_probe = TcpListener::bind("127.0.0.1:0").expect("failed to reserve server port");
    let server_port = server_probe.local_addr().expect("server probe addr").port() as i32;
    drop(server_probe);

    fs::write(
        &source_path,
        format!(
            r#"from network import tcp_client

def main() -> i32:
    let client = tcp_client("127.0.0.1", {0})
    let handle: i32 = client.open_handle(1000)
    println(client.send_line_handle(handle, "ping"))
    println(client.recv_handle(handle, 64, 1000))
    println(client.close_handle(handle))
    return 0
"#,
            server_port
        ),
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None)
        .expect("persistent network client class program should build");

    let _server = spawn_tcp_request_server_on_port(server_port as u16, b"pong\n");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run persistent network client class program");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "true\npong\n\ntrue\n");
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

#[test]
fn builds_and_runs_stdlib_fs_extended_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_fs_extended.rn");
    let exe_path = dir.join("stdlib_fs_extended.exe");
    let nested_dir = dir.join("nested");
    let file_path = nested_dir.join("note.txt");
    let rune_dir = nested_dir.display().to_string().replace('\\', "/");
    let rune_file = file_path.display().to_string().replace('\\', "/");

    fs::write(
        &source_path,
        format!(
            r#"from fs import append_string, canonicalize, chdir, create_dir_all, current_dir, file_size, is_dir, is_file, read_string, set_current_dir, write_string

def main() -> i32:
    println(create_dir_all("{0}"))
    println(is_dir("{0}"))
    println(write_string("{1}", "abc"))
    println(append_string("{1}", "def"))
    println(read_string("{1}"))
    println(file_size("{1}"))
    println(is_file("{1}"))
    let before: String = current_dir()
    println(canonicalize("{0}") != "")
    println(set_current_dir("{0}"))
    println(chdir(before))
    println(current_dir() == before)
    return 0
"#,
            rune_dir, rune_file
        ),
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("extended fs program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run extended fs executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "true\ntrue\ntrue\ntrue\nabcdef\n6\ntrue\ntrue\ntrue\ntrue\ntrue\n");
}

#[test]
fn builds_and_runs_string_find_program() {
    let dir = temp_dir();
    let source_path = dir.join("string_find.rn");
    let exe_path = dir.join("string_find.exe");

    fs::write(
        &source_path,
        r#"def main() -> i32:
    let s: String = "hello world"
    let idx: i64 = s.find("world")
    println(idx)
    let miss: i64 = s.find("xyz")
    println(miss)
    let empty: i64 = s.find("")
    println(empty)
    return 0
"#,
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("string find program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run string find executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "6\n-1\n0\n");
}

#[test]
fn builds_and_runs_string_slice_program() {
    let dir = temp_dir();
    let source_path = dir.join("string_slice.rn");
    let exe_path = dir.join("string_slice.exe");

    fs::write(
        &source_path,
        r#"def main() -> i32:
    let s: String = "hello world"
    let start: i64 = 6
    let end: i64 = 11
    let sub: String = s.slice(start, end)
    println(sub)
    let prefix: String = s.slice(0, 5)
    println(prefix)
    return 0
"#,
    )
    .expect("failed to write source");

    build_executable(&source_path, &exe_path, None).expect("string slice program should build");

    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run string slice executable");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "world\nhello\n");
}
