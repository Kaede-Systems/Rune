use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use rune::build::build_executable;

fn temp_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rune-stdlib-runtime-test-{stamp}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

#[test]
fn builds_and_runs_stdlib_env_fs_system_time_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_env_fs_system_time.rn");
    let exe_path = dir.join("stdlib_env_fs_system_time.exe");
    let file_path = dir.join("note.txt");
    let rune_file_path = file_path.display().to_string().replace('\\', "/");

    let source = format!(
        r#"from env import get_i32_or_zero, get_bool_or_false
from fs import exists, read_string, write_text
from system import pid, cpu_count
from time import monotonic_ms, sleep_until

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
    println(get_i32_or_zero("RUNE_STDLIB_INT"))
    if get_bool_or_false("RUNE_STDLIB_BOOL"):
        println("bool-yes")
    else:
        println("bool-no")
    let start: i64 = monotonic_ms()
    sleep_until(start)
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
        .env("RUNE_STDLIB_INT", "17")
        .env("RUNE_STDLIB_BOOL", "true")
        .output()
        .expect("failed to run stdlib helper program");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(
        stdout,
        "missing-before\nwrite-ok\nhello stdlib\n17\nbool-yes\npid-ok\ncpu-ok\n"
    );
    let written = fs::read_to_string(&file_path).expect("stdlib write_text should create file");
    assert_eq!(written, "hello stdlib");
}

#[test]
fn builds_and_runs_stdlib_terminal_and_audio_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_terminal_audio.rn");
    let exe_path = dir.join("stdlib_terminal_audio.exe");

    fs::write(
        &source_path,
        r#"from terminal import hide, show, clear_and_home
from audio import beep

def main() -> i32:
    hide()
    show()
    clear_and_home()
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
    assert!(text.ends_with("\x071\n"), "unexpected terminal/audio output: {text:?}");
}

#[test]
fn builds_and_runs_stdlib_network_alias_program() {
    let dir = temp_dir();
    let source_path = dir.join("stdlib_network_alias.rn");
    let exe_path = dir.join("stdlib_network_alias.exe");

    fs::write(
        &source_path,
        r#"from network import tcp_probe, tcp_probe_timeout

def main() -> i32:
    println(tcp_probe("127.0.0.1", 65535))
    println(tcp_probe_timeout("127.0.0.1", 65535, 1))
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
    assert_eq!(stdout, "0\n0\n");
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
