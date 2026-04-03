use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use rune::toolchain::find_packaged_llvm_avr_tool;

fn temp_dir() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rune-cli-omission-test-{stamp}"));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

#[test]
fn emit_llvm_asm_omits_unreachable_helpers_for_executables() {
    let dir = temp_dir();
    let path = dir.join("pruned_exec.rn");
    fs::write(
        &path,
        "def live() -> i32:\n    return 1\n\n\
         def dead_helper() -> i32:\n    return 7\n\n\
         def main() -> i32:\n    println(live())\n    return 0\n",
    )
    .expect("failed to write source");

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("emit-llvm-asm")
        .arg(&path)
        .output()
        .expect("failed to run rune emit-llvm-asm");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("live"));
    assert!(!stdout.contains("dead_helper"));
}

#[test]
fn emit_llvm_asm_omits_unused_stdlib_fs_wrappers_and_runtime_hooks() {
    let dir = temp_dir();
    let path = dir.join("pruned_stdlib_fs.rn");
    fs::write(
        &path,
        "from fs import exists, remove\n\n\
         def main() -> i32:\n    println(exists(\"note.txt\"))\n    return 0\n",
    )
    .expect("failed to write source");

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("emit-llvm-asm")
        .arg(&path)
        .output()
        .expect("failed to run rune emit-llvm-asm");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("exists"));
    assert!(stdout.contains("rune_rt_fs_exists"));
    assert!(!stdout.contains("remove"));
    assert!(!stdout.contains("rune_rt_fs_remove"));
}

#[test]
fn emit_llvm_asm_omits_unused_stdlib_network_wrappers_and_runtime_hooks() {
    let dir = temp_dir();
    let path = dir.join("pruned_stdlib_network.rn");
    fs::write(
        &path,
        "from network import connect, send_line\n\n\
         def main() -> i32:\n    println(connect(\"127.0.0.1\", 65535))\n    return 0\n",
    )
    .expect("failed to write source");

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("emit-llvm-asm")
        .arg(&path)
        .output()
        .expect("failed to run rune emit-llvm-asm");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("tcp_connect"));
    assert!(stdout.contains("rune_rt_network_tcp_connect"));
    assert!(!stdout.contains("send_line"));
    assert!(!stdout.contains("rune_rt_network_tcp_send"));
}

#[test]
fn emit_asm_supports_avr_target_when_avr_llvm_tools_are_available() {
    if find_packaged_llvm_avr_tool("llc").is_none() {
        return;
    }

    let dir = temp_dir();
    let path = dir.join("avr_emit_asm.rn");
    fs::write(
        &path,
        "println(\"Hello from AVR asm\")\n",
    )
    .expect("failed to write source");

    let output = Command::new(env!("CARGO_BIN_EXE_rune"))
        .arg("emit-asm")
        .arg(&path)
        .arg("--target")
        .arg("avr-atmega328p-arduino-uno")
        .output()
        .expect("failed to run rune emit-asm for avr");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("push"));
    assert!(stdout.contains("call"));
    assert!(stdout.contains(".type\tmain,@function") || stdout.contains(".type\tmain, @function") || stdout.contains(".type\tmain,@function"));
}
