use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rune-cli-wasm-test-{stamp}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

fn wasm_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn run_wasm_command_supports_node_host_for_unknown_unknown_modules() {
    let _guard = wasm_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source = dir.join("cli_host_demo.rn");
    let wasm = dir.join("cli_host_demo.wasm");
    fs::write(
        &source,
        "def main() -> i32:\n    println(\"hello wasm hosts\")\n    return 0\n",
    )
    .expect("failed to write source");

    let build_output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("build")
        .arg(&source)
        .arg("--target")
        .arg("wasm32-unknown-unknown")
        .arg("-o")
        .arg(&wasm)
        .output()
        .expect("failed to build wasm module");
    assert!(
        build_output.status.success(),
        "build stderr: {}",
        String::from_utf8_lossy(&build_output.stderr)
    );

    let node_output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("run-wasm")
        .arg(&wasm)
        .arg("--host")
        .arg("node")
        .output()
        .expect("failed to run rune run-wasm via node");
    assert!(
        node_output.status.success(),
        "node stderr: {}",
        String::from_utf8_lossy(&node_output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&node_output.stdout).replace("\r\n", "\n"),
        "hello wasm hosts\n"
    );
}

#[test]
fn run_wasm_command_supports_direct_wasmtime_for_wasi_modules() {
    let _guard = wasm_lock()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let dir = temp_dir();
    let source = dir.join("cli_wasi_demo.rn");
    let wasm = dir.join("cli_wasi_demo.wasm");
    fs::write(
        &source,
        "def main() -> i32:\n    println(\"hello wasi\")\n    return 7\n",
    )
    .expect("failed to write source");

    let build_output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("build")
        .arg(&source)
        .arg("--target")
        .arg("wasm32-wasip1")
        .arg("-o")
        .arg(&wasm)
        .output()
        .expect("failed to build wasi module");
    assert!(
        build_output.status.success(),
        "build stderr: {}",
        String::from_utf8_lossy(&build_output.stderr)
    );

    let wasmtime_output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("run-wasm")
        .arg(&wasm)
        .arg("--host")
        .arg("wasmtime")
        .output()
        .expect("failed to run rune run-wasm via direct wasmtime");
    assert_eq!(wasmtime_output.status.code(), Some(7));
    assert_eq!(
        String::from_utf8_lossy(&wasmtime_output.stdout).replace("\r\n", "\n"),
        "hello wasi\n"
    );
}
