use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use rune::build::build_executable;
use rune::toolchain::find_packaged_wasmtime;

fn temp_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rune-wasm-runtime-test-{stamp}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

fn wasm_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn wasm_build_generates_loader_and_runs_print_program_in_node() {
    let _guard = wasm_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("print_main.rn");
    let wasm_path = dir.join("print_main.wasm");
    let runner_path = dir.join("run_print.js");

    fs::write(
        &source_path,
        "def main() -> i32:\n    print(\"sum=\")\n    println(42)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &wasm_path, Some("wasm32-unknown-unknown"))
        .expect("wasm print build should succeed");

    let loader_path = wasm_path.with_extension("js");
    assert!(
        loader_path.is_file(),
        "expected wasm loader sidecar to exist"
    );

    fs::write(
        &runner_path,
        format!(
            "const {{ instantiateRuneWasm }} = require({:?});\n(async () => {{\n  const runtime = await instantiateRuneWasm({:?});\n  const result = runtime.runMain();\n  process.stdout.write(\"ret=\" + result.toString() + \"\\n\");\n}})().catch((error) => {{ console.error(error.stack || String(error)); process.exit(1); }});\n",
            loader_path.to_string_lossy().to_string(),
            wasm_path.to_string_lossy().to_string(),
        ),
    )
    .expect("failed to write node runner");

    let output = Command::new("node")
        .arg(&runner_path)
        .output()
        .expect("failed to run node wasm print runner");

    assert!(
        output.status.success(),
        "node stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "sum=42\nret=0\n");
}

#[test]
fn wasi_build_runs_print_program_in_packaged_wasmtime() {
    let _guard = wasm_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("print_wasi_main.rn");
    let wasm_path = dir.join("print_py_main.wasm");

    fs::write(
        &source_path,
        "def main() -> i32:\n    print(\"sum=\")\n    println(42)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &wasm_path, Some("wasm32-wasip1"))
        .expect("wasi print build should succeed");

    let wasmtime = find_packaged_wasmtime().expect("packaged wasmtime.exe should exist");
    let output = Command::new(wasmtime)
        .arg(&wasm_path)
        .output()
        .expect("failed to run packaged wasmtime");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "sum=42\n");
}

#[test]
fn wasm_build_runs_input_program_in_node() {
    let _guard = wasm_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("input_main.rn");
    let wasm_path = dir.join("input_main.wasm");
    let runner_path = dir.join("run_input.js");

    fs::write(
        &source_path,
        "def main() -> i32:\n    let line: String = input()\n    println(line)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &wasm_path, Some("wasm32-unknown-unknown"))
        .expect("wasm input build should succeed");

    let loader_path = wasm_path.with_extension("js");
    fs::write(
        &runner_path,
        format!(
            "const {{ instantiateRuneWasm }} = require({:?});\n(async () => {{\n  const runtime = await instantiateRuneWasm({:?});\n  runtime.runMain();\n}})().catch((error) => {{ console.error(error.stack || String(error)); process.exit(1); }});\n",
            loader_path.to_string_lossy().to_string(),
            wasm_path.to_string_lossy().to_string(),
        ),
    )
    .expect("failed to write node runner");

    let mut child = Command::new("node")
        .arg(&runner_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start node wasm input runner");

    child
        .stdin
        .as_mut()
        .expect("stdin should be piped")
        .write_all(b"hello wasm\n")
        .expect("failed to write stdin");

    let output = child
        .wait_with_output()
        .expect("failed to collect node wasm input output");

    assert!(
        output.status.success(),
        "node stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "hello wasm\n");
}

#[test]
fn wasm_build_runs_panic_program_in_node() {
    let _guard = wasm_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("panic_main.rn");
    let wasm_path = dir.join("panic_main.wasm");
    let runner_path = dir.join("run_panic.js");

    fs::write(&source_path, "def main() -> i32:\n    panic(\"boom\")\n")
        .expect("failed to write source");

    build_executable(&source_path, &wasm_path, Some("wasm32-unknown-unknown"))
        .expect("wasm panic build should succeed");

    let loader_path = wasm_path.with_extension("js");
    fs::write(
        &runner_path,
        format!(
            "const {{ instantiateRuneWasm }} = require({:?});\n(async () => {{\n  const runtime = await instantiateRuneWasm({:?});\n  runtime.runMain();\n}})().catch((error) => {{ console.error(error.message || String(error)); process.exit(1); }});\n",
            loader_path.to_string_lossy().to_string(),
            wasm_path.to_string_lossy().to_string(),
        ),
    )
    .expect("failed to write node runner");

    let output = Command::new("node")
        .arg(&runner_path)
        .output()
        .expect("failed to run node wasm panic runner");

    assert!(!output.status.success(), "panic runner should fail");
    let stderr = String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n");
    assert!(stderr.contains("Rune panic: boom"));
    assert!(stderr.contains("panic in main"));
}

#[test]
fn wasm_build_runs_stdlib_builtin_program_in_node() {
    let _guard = wasm_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("stdlib_main.rn");
    let wasm_path = dir.join("stdlib_main.wasm");
    let runner_path = dir.join("run_stdlib.js");

    fs::write(
        &source_path,
        "def main() -> i32:\n    println(__rune_builtin_system_pid())\n    println(__rune_builtin_system_cpu_count())\n    println(__rune_builtin_env_arg_count())\n    println(__rune_builtin_env_exists(\"RUNE_WASM_FLAG\"))\n    println(__rune_builtin_env_get_i32(\"RUNE_WASM_PORT\", 8080))\n    println(__rune_builtin_env_get_bool(\"RUNE_WASM_BOOL\", false))\n    println(__rune_builtin_env_get_string(\"RUNE_WASM_HOST\", \"fallback-host\"))\n    println(__rune_builtin_network_tcp_connect_timeout(\"127.0.0.1\", 65535, 10))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &wasm_path, Some("wasm32-unknown-unknown"))
        .expect("wasm stdlib build should succeed");

    let loader_path = wasm_path.with_extension("js");
    fs::write(
        &runner_path,
        format!(
            "process.env.RUNE_WASM_FLAG = '1';\nprocess.env.RUNE_WASM_PORT = '9091';\nprocess.env.RUNE_WASM_BOOL = 'true';\nprocess.env.RUNE_WASM_HOST = 'node-host';\nconst {{ instantiateRuneWasm }} = require({:?});\n(async () => {{\n  const runtime = await instantiateRuneWasm({:?});\n  runtime.runMain();\n}})().catch((error) => {{ console.error(error.stack || String(error)); process.exit(1); }});\n",
            loader_path.to_string_lossy().to_string(),
            wasm_path.to_string_lossy().to_string(),
        ),
    )
    .expect("failed to write node runner");

    let output = Command::new("node")
        .arg(&runner_path)
        .arg("alpha")
        .arg("beta")
        .output()
        .expect("failed to run node wasm stdlib runner");

    assert!(
        output.status.success(),
        "node stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 8, "unexpected stdout: {stdout}");
    assert!(lines[0].parse::<i32>().unwrap_or_default() > 0);
    assert!(lines[1].parse::<i32>().unwrap_or_default() >= 1);
    assert_eq!(lines[2], "2");
    assert_eq!(lines[3], "true");
    assert_eq!(lines[4], "9091");
    assert_eq!(lines[5], "true");
    assert_eq!(lines[6], "node-host");
    assert_eq!(lines[7], "false");
}

#[test]
fn wasi_build_runs_stdlib_builtin_program_in_packaged_wasmtime() {
    let _guard = wasm_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("stdlib_main_wasi.rn");
    let wasm_path = dir.join("stdlib_main_py.wasm");

    fs::write(
        &source_path,
        "def main() -> i32:\n    println(__rune_builtin_system_pid())\n    println(__rune_builtin_system_cpu_count())\n    println(__rune_builtin_env_arg_count())\n    println(__rune_builtin_env_exists(\"RUNE_WASM_FLAG\"))\n    println(__rune_builtin_env_get_i32(\"RUNE_WASM_PORT\", 8080))\n    println(__rune_builtin_env_get_bool(\"RUNE_WASM_BOOL\", false))\n    println(__rune_builtin_env_get_string(\"RUNE_WASM_HOST\", \"fallback-host\"))\n    println(__rune_builtin_network_tcp_connect_timeout(\"127.0.0.1\", 65535, 10))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &wasm_path, Some("wasm32-wasip1"))
        .expect("wasi stdlib build should succeed");

    let wasmtime = find_packaged_wasmtime().expect("packaged wasmtime.exe should exist");
    let output = Command::new(wasmtime)
        .arg("run")
        .arg("--argv0")
        .arg("rune-wasi")
        .arg("--env")
        .arg("RUNE_WASM_FLAG")
        .arg("--env")
        .arg("RUNE_WASM_PORT")
        .arg("--env")
        .arg("RUNE_WASM_BOOL")
        .arg("--env")
        .arg("RUNE_WASM_HOST")
        .arg(&wasm_path)
        .arg("alpha")
        .arg("beta")
        .env("RUNE_WASM_FLAG", "1")
        .env("RUNE_WASM_PORT", "9091")
        .env("RUNE_WASM_BOOL", "true")
        .env("RUNE_WASM_HOST", "wasi-host")
        .output()
        .expect("failed to run packaged wasmtime stdlib runner");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 8, "unexpected stdout: {stdout}");
    assert!(lines[0].parse::<i32>().unwrap_or_default() > 0);
    assert!(lines[1].parse::<i32>().unwrap_or_default() >= 1);
    assert_eq!(lines[2], "2");
    assert_eq!(lines[3], "true");
    assert_eq!(lines[4], "9091");
    assert_eq!(lines[5], "true");
    assert_eq!(lines[6], "wasi-host");
    assert_eq!(lines[7], "false");
}

#[test]
fn wasm_build_runs_time_microsecond_program_in_node() {
    let _guard = wasm_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("time_micro_node.rn");
    let wasm_path = dir.join("time_micro_node.wasm");
    let runner_path = dir.join("run_time_micro.js");

    fs::write(
        &source_path,
        "from time import monotonic_us, sleep_us, sleep_until_us\n\n\
         def main() -> i32:\n    let start: i64 = monotonic_us()\n    sleep_us(1000)\n    sleep_until_us(monotonic_us())\n    println(monotonic_us() >= start)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &wasm_path, Some("wasm32-unknown-unknown"))
        .expect("wasm time micro build should succeed");

    let loader_path = wasm_path.with_extension("js");
    fs::write(
        &runner_path,
        format!(
            "const {{ instantiateRuneWasm }} = require({:?});\n(async () => {{\n  const runtime = await instantiateRuneWasm({:?});\n  runtime.runMain();\n}})().catch((error) => {{ console.error(error.stack || String(error)); process.exit(1); }});\n",
            loader_path.to_string_lossy().to_string(),
            wasm_path.to_string_lossy().to_string(),
        ),
    )
    .expect("failed to write node runner");

    let output = Command::new("node")
        .arg(&runner_path)
        .output()
        .expect("failed to run node wasm time runner");

    assert!(
        output.status.success(),
        "node stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "true\n");
}

#[test]
fn wasi_build_runs_time_microsecond_program_in_packaged_wasmtime() {
    let _guard = wasm_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("time_micro_wasi.rn");
    let wasm_path = dir.join("time_micro_wasi.wasm");

    fs::write(
        &source_path,
        "from time import monotonic_us, sleep_us, sleep_until_us\n\n\
         def main() -> i32:\n    let start: i64 = monotonic_us()\n    sleep_us(1000)\n    sleep_until_us(monotonic_us())\n    println(monotonic_us() >= start)\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &wasm_path, Some("wasm32-wasip1"))
        .expect("wasi time micro build should succeed");

    let wasmtime = find_packaged_wasmtime().expect("packaged wasmtime.exe should exist");
    let output = Command::new(wasmtime)
        .arg(&wasm_path)
        .output()
        .expect("failed to run packaged wasmtime");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "true\n");
}

#[test]
fn wasm_build_runs_network_stdlib_program_in_node() {
    let _guard = wasm_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("network_stdlib_node.rn");
    let wasm_path = dir.join("network_stdlib_node.wasm");
    let runner_path = dir.join("run_network_stdlib.js");

    fs::write(
        &source_path,
        "from network import connect_timeout, tcp_client\n\n\
         def main() -> i32:\n    let client = tcp_client(\"127.0.0.1\", 65535)\n    println(connect_timeout(\"127.0.0.1\", 65535, 10))\n    println(client.probe_timeout(10))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &wasm_path, Some("wasm32-unknown-unknown"))
        .expect("wasm network stdlib build should succeed");

    let loader_path = wasm_path.with_extension("js");
    fs::write(
        &runner_path,
        format!(
            "const {{ instantiateRuneWasm }} = require({:?});\n(async () => {{\n  const runtime = await instantiateRuneWasm({:?});\n  runtime.runMain();\n}})().catch((error) => {{ console.error(error.stack || String(error)); process.exit(1); }});\n",
            loader_path.to_string_lossy().to_string(),
            wasm_path.to_string_lossy().to_string(),
        ),
    )
    .expect("failed to write node runner");

    let output = Command::new("node")
        .arg(&runner_path)
        .output()
        .expect("failed to run node wasm network stdlib runner");

    assert!(
        output.status.success(),
        "node stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "false\nfalse\n");
}

#[test]
fn wasi_build_runs_network_stdlib_program_in_packaged_wasmtime() {
    let _guard = wasm_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("network_stdlib_wasi.rn");
    let wasm_path = dir.join("network_stdlib_wasi.wasm");

    fs::write(
        &source_path,
        "from network import connect_timeout, tcp_client\n\n\
         def main() -> i32:\n    let client = tcp_client(\"127.0.0.1\", 65535)\n    println(connect_timeout(\"127.0.0.1\", 65535, 10))\n    println(client.probe_timeout(10))\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &wasm_path, Some("wasm32-wasip1"))
        .expect("wasi network stdlib build should succeed");

    let wasmtime = find_packaged_wasmtime().expect("packaged wasmtime.exe should exist");
    let output = Command::new(wasmtime)
        .arg(&wasm_path)
        .output()
        .expect("failed to run packaged wasmtime");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert_eq!(stdout, "false\nfalse\n");
}

#[test]
fn wasm_build_runs_fs_terminal_and_audio_program_in_node() {
    let _guard = wasm_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("fs_terminal_audio_node.rn");
    let wasm_path = dir.join("fs_terminal_audio_node.wasm");
    let runner_path = dir.join("run_fs_terminal_audio.js");
    let file_path = dir.join("node_note.txt");

    fs::write(
        &source_path,
        format!(
            "def main() -> i32:\n    println(__rune_builtin_fs_exists({:?}))\n    println(__rune_builtin_fs_write_string({:?}, \"hello wasm\"))\n    println(__rune_builtin_fs_read_string({:?}))\n    __rune_builtin_terminal_clear()\n    __rune_builtin_terminal_move_to(1, 1)\n    __rune_builtin_terminal_hide_cursor()\n    __rune_builtin_terminal_set_title(\"Rune WASM\")\n    __rune_builtin_terminal_show_cursor()\n    println(__rune_builtin_audio_bell())\n    return 0\n",
            file_path.to_string_lossy().to_string(),
            file_path.to_string_lossy().to_string(),
            file_path.to_string_lossy().to_string()
        ),
    )
    .expect("failed to write source");

    build_executable(&source_path, &wasm_path, Some("wasm32-unknown-unknown"))
        .expect("wasm fs/terminal/audio build should succeed");

    let loader_path = wasm_path.with_extension("js");
    fs::write(
        &runner_path,
        format!(
            "const {{ instantiateRuneWasm }} = require({:?});\n(async () => {{\n  const runtime = await instantiateRuneWasm({:?});\n  runtime.runMain();\n}})().catch((error) => {{ console.error(error.stack || String(error)); process.exit(1); }});\n",
            loader_path.to_string_lossy().to_string(),
            wasm_path.to_string_lossy().to_string(),
        ),
    )
    .expect("failed to write node runner");

    let output = Command::new("node")
        .arg(&runner_path)
        .output()
        .expect("failed to run node wasm fs/terminal/audio runner");

    assert!(
        output.status.success(),
        "node stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert!(stdout.contains("false\ntrue\nhello wasm\n"), "unexpected stdout: {stdout}");
    assert!(stdout.contains("true\n"), "unexpected stdout: {stdout}");
    let file_contents = fs::read_to_string(&file_path).expect("node wasm should write file");
    assert_eq!(file_contents, "hello wasm");
}

#[test]
fn wasi_build_runs_fs_terminal_and_audio_program_in_packaged_wasmtime() {
    let _guard = wasm_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source_path = dir.join("fs_terminal_audio_wasi.rn");
    let wasm_path = dir.join("fs_terminal_audio_wasi.wasm");

    fs::write(
        &source_path,
        "def main() -> i32:\n    println(__rune_builtin_fs_exists(\"note.txt\"))\n    println(__rune_builtin_fs_write_string(\"note.txt\", \"hello wasi\"))\n    println(__rune_builtin_fs_read_string(\"note.txt\"))\n    __rune_builtin_terminal_clear()\n    __rune_builtin_terminal_move_to(1, 1)\n    __rune_builtin_terminal_hide_cursor()\n    __rune_builtin_terminal_set_title(\"Rune WASI\")\n    __rune_builtin_terminal_show_cursor()\n    println(__rune_builtin_audio_bell())\n    return 0\n",
    )
    .expect("failed to write source");

    build_executable(&source_path, &wasm_path, Some("wasm32-wasip1"))
        .expect("wasi fs/terminal/audio build should succeed");

    let wasmtime = find_packaged_wasmtime().expect("packaged wasmtime.exe should exist");
    let output = Command::new(wasmtime)
        .arg("run")
        .arg("--dir")
        .arg(format!("{}::.", dir.display()))
        .arg(&wasm_path)
        .current_dir(&dir)
        .output()
        .expect("failed to run packaged wasmtime fs/terminal/audio runner");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    assert!(stdout.contains("false\ntrue\nhello wasi\n"), "unexpected stdout: {stdout}");
    assert!(stdout.contains("true\n"), "unexpected stdout: {stdout}");
    let file_contents = fs::read_to_string(dir.join("note.txt")).expect("wasi should write file");
    assert_eq!(file_contents, "hello wasi");
}
